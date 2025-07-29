use std::process::Command;
use serde::Deserialize;
use ipnet::IpNet;
use log;
use reticulum::destination::link::LinkEvent;
use reticulum::hash::AddressHash;
use reticulum::transport::Transport;
use riptun::TokioTun;

// TODO: config?
const TUN_NQUEUES : usize = 1;
const MTU: usize = 1500;

pub struct Client {
  config: ClientConfig,
  tun: Tun
}

pub struct Server {
  config: ServerConfig,
  tun: Tun
}

pub struct Tun {
  tun: TokioTun,
  read_buf: tokio::sync::Mutex<[u8; MTU]>
}

#[derive(Deserialize)]
pub struct ClientConfig {
  pub log_level: String,
  pub tun_ip: IpNet,
  pub target_ip: IpNet,
  // TODO: deserialize AddressHash
  pub server_destination: String
}

#[derive(Deserialize)]
pub struct ServerConfig {
  pub log_level: String,
  pub tun_ip: IpNet,
  pub client_subnet: IpNet,
  pub outbound_interface: String,
  // TODO: deserialize AddressHash
  pub client_destination: String
}

#[derive(Debug)]
pub enum CreateAdapterError {
  RiptunError(riptun::Error),
  IpAddBroadcastError(std::io::Error),
  IpLinkUpError(std::io::Error),
  IpRouteAddError(std::io::Error),
  IptablesError(std::io::Error)
}

impl Client {
  pub fn new(config: ClientConfig) -> Result<Self, CreateAdapterError> {
    let tun = Tun::new(config.tun_ip)?;
    // configure forwarding to tun for target IP
    // ip route add <target-ip> dev rip0
    log::info!("adding route for {}", config.target_ip);
    let output = Command::new("ip")
      .arg("route")
      .arg("add")
      .arg(config.target_ip.to_string())
      .arg("dev")
      .arg(tun.tun().name())
      .output()
      .map_err(|err| {
        log::error!("error adding route for {}: {err:?}", config.target_ip);
        CreateAdapterError::IpRouteAddError(err)
      })?;
    if !output.status.success() {
      let err_s = format!("ip route add command failed ({:?})", output.status.code());
      log::error!("{}", err_s);
      return Err(CreateAdapterError::IpRouteAddError(std::io::Error::other(err_s)))
    }
    Ok(Client { config, tun })
  }

  pub async fn run(&self, transport: Transport) {
    // set up links
    let link_loop = async || {
      let server_destination =
        match AddressHash::new_from_hex_string(self.config.server_destination.as_str()) {
          Ok(dest) => dest,
          Err(err) => {
            log::error!("error parsing server destination hash: {err:?}");
            return
          }
        };
      let mut announce_recv = transport.recv_announces().await;
      // TODO: continue looping after link is created?
      while let Ok(announce) = announce_recv.recv().await {
        let destination = announce.destination.lock().await;
        if destination.desc.address_hash == server_destination {
          let _link = transport.link(destination.desc).await;
        }
      }
    };
    // listen to tun and forward to links
    let read_tun_loop = async || while let Ok(bytes) = self.tun.read().await {
        log::trace!("got tun bytes ({})", bytes.len());
        transport.send_to_all_out_links(bytes.as_slice()).await;
    };
    // forward upstream link messages to tun
    let write_tun_loop = async || {
      let mut out_link_events = transport.out_link_events();
      while let Ok(link_event) = out_link_events.recv().await {
        match link_event.event {
          LinkEvent::Data(payload) => {
            log::trace!("link {} payload ({})", link_event.id, payload.len());
            match self.tun.send(payload.as_slice()).await {
              Ok(n) => log::trace!("tun sent {n} bytes"),
              Err(err) => {
                log::error!("tun error sending bytes: {err:?}");
                break
              }
            }
          }
          _ => {}
        }
      }
    };
    // run
    tokio::select!{
      _ = read_tun_loop() => log::info!("read tun loop exited: shutting down"),
      _ = write_tun_loop() => log::info!("write tun loop exited: shutting down"),
      _ = link_loop() => log::info!("link loop exited: shutting down"),
      _ = tokio::signal::ctrl_c() => log::info!("got ctrl-c: shutting down")
    }
  }
}

impl Server {
  pub fn new(config: ServerConfig) -> Result<Self, CreateAdapterError> {
    let tun = Tun::new(config.tun_ip)?;
    // add nat rule to masquerade as client
    log::info!("adding nat masquerade for {}", config.client_subnet);
    let output = Command::new("iptables")
      .arg("-t")
      .arg("nat")
      .arg("-A")
      .arg("POSTROUTING")
      .arg("-s")
      .arg(config.client_subnet.to_string())
      .arg("-o")
      .arg(config.outbound_interface.to_string())
      .arg("-j")
      .arg("MASQUERADE")
      .output()
      .map_err(|err|{
        log::error!("error adding nat masquerade for {}: {err:?}", config.client_subnet);
        CreateAdapterError::IptablesError(err)
      })?;
    if !output.status.success() {
      let err_s = format!("iptables nat masquerade command failed ({:?})", output.status.code());
      log::error!("{}", err_s);
      return Err(CreateAdapterError::IptablesError(std::io::Error::other(err_s)))
    }
    Ok(Server { config, tun })
  }

  pub async fn run(&self) {
    unimplemented!("TODO")
  }
}

impl Tun {
  pub fn new(ip: IpNet) -> Result<Self, CreateAdapterError> {
    log::debug!("creating tun device");
    let ip: IpNet = ip.into();
    let tun = TokioTun::new("rip%d", TUN_NQUEUES).map_err(CreateAdapterError::RiptunError)?;
    log::debug!("created tun device: {}", tun.name());
    log::debug!("adding broadcast ip addr: {}", ip);
    let output = std::process::Command::new("ip")
      .arg("addr")
      .arg("add")
      .arg(ip.to_string())
      .arg("brd")
      .arg(ip.addr().to_string())
      .arg("dev")
      .arg(tun.name())
      .output()
      .map_err(CreateAdapterError::IpAddBroadcastError)?;
    if !output.status.success() {
      return Err(CreateAdapterError::IpAddBroadcastError(
        std::io::Error::other(format!("ip addr add command failed ({:?})",
          output.status.code())).into()));
    }
    log::debug!("{} setting link up", tun.name());
    let output = std::process::Command::new("ip")
      .arg("link")
      .arg("set")
      .arg("dev")
      .arg(tun.name())
      .arg("up")
      .output()
      .map_err(CreateAdapterError::IpLinkUpError)?;
    if !output.status.success() {
      return Err(CreateAdapterError::IpLinkUpError(
        std::io::Error::other(format!("ip link set command failed ({:?})", output.status.code()))))
    }
    let adapter = Tun {
      tun, read_buf: tokio::sync::Mutex::new([0x0; MTU])
    };
    Ok(adapter)
  }

  pub fn tun(&self) -> &TokioTun {
    &self.tun
  }

  // TODO: can we return a lock of &[u8] to avoid creating vec?
  pub async fn read(&self) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = self.read_buf.lock().await;
    let nbytes = self.tun.recv(&mut buf[..]).await?;
    Ok(buf[..nbytes].to_vec())
  }

  pub async fn send(&self, datagram: &[u8]) -> Result<usize, std::io::Error> {
    self.tun.send(datagram).await
  }
}
