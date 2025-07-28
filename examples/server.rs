use std::sync::Arc;
use env_logger;
use log;
use tokio;

use reticulum::destination::DestinationName;
use reticulum::destination::link::{LinkEvent, LinkId};
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    log::info!("server start");
    // tun adapter
    let adapter = match rns_tun::Adapter::new("10.0.0.1", 24) {
      Ok(adapter) => adapter,
      Err(err) => match err {
        riptun::Error::Unix { source: nix::errno::Errno::EPERM } => {
          log::error!("EPERM error creating TUN interface: need to run with root permissions");
          std::process::exit(1)
        }
        _ => {
          log::error!("error creating TUN interface: {:?}", err);
          std::process::exit(1)
        }
      }
    };
    // add nat rule to masquerade as A
    // TODO: command-line arg for interface wlp3s0 or detect outbound network interface?
    let client_subnet = "10.0.0.0/24";
    log::info!("adding nat masquerade for {client_subnet}");
    let output = match std::process::Command::new("iptables")
      .arg("-t")
      .arg("nat")
      .arg("-A")
      .arg("POSTROUTING")
      .arg("-s")
      .arg(client_subnet)
      .arg("-o")
      .arg("wlp3s0")
      .arg("-j")
      .arg("MASQUERADE")
      .output()
    {
      Ok(output) => output,
      Err(err) => {
        log::error!("error adding nat masquerade for {client_subnet}: {err:?}");
        std::process::exit(1)
      }
    };
    if !output.status.success() {
      log::error!("iptables nat masquerade command failed ({:?})", output.status.code());
      std::process::exit(1)
    }
    // start reticulum
    log::info!("starting reticulum");
    let id = PrivateIdentity::new_from_name("server");
    let mut transport = Transport::new(TransportConfig::new("server", &id, true));
    let _ = transport.iface_manager().lock().await.spawn(
        TcpServer::new("0.0.0.0:4242", transport.iface_manager()),
        TcpServer::spawn,
    );
    let in_destination = transport
      .add_destination(id, DestinationName::new("example", "server")).await;
    // send announces
    let announce_loop = async || loop {
      transport.send_announce(&in_destination, None).await;
      tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    };
    let link_id: Arc<tokio::sync::Mutex<Option<LinkId>>> = Arc::new(tokio::sync::Mutex::new(None));
    // tun loop
    let tun_loop = async || while let Ok(bytes) = adapter.read().await {
      log::trace!("got tun bytes ({})", bytes.len());
      /*FIXME:debug*/
      {
        use std::io::Write;
        let mut file = std::fs::File::create("icmp-packet").unwrap();
        file.write_all(bytes.as_slice()).unwrap();
      }
      let link_id = link_id.lock().await;
      if let Some(link_id) = link_id.as_ref() {
        log::trace!("sending on link ({})", link_id);
        let link = transport.find_in_link(link_id).await.unwrap();
        let link = link.lock().await;
        let packet = link.data_packet(&bytes).unwrap();
        transport.send_packet(packet).await;
      }
    };
    // upstream link data
    let link_loop = async || {
      let mut in_link_events = transport.in_link_events();
      while let Ok(link_event) = in_link_events.recv().await {
        match link_event.event {
          LinkEvent::Data(payload) => {
            /*FIXME:debug*/ println!("LINK DATA");
            log::trace!("link {} payload ({})", link_event.id, payload.len());
            match adapter.send(payload.as_slice()).await {
              Ok(n) => log::trace!("tun sent {n} bytes"),
              Err(err) => {
                log::error!("tun error sending bytes: {err:?}");
                break
              }
            }
          }
          /*FIXME:debug*/
          LinkEvent::Activated => {
            println!("LINK ACTIVATED");
            let mut link_id = link_id.lock().await;
            *link_id = Some(link_event.id);
          }
          LinkEvent::Closed => println!("LINK CLOSED"),
          //_ => {}
        }
      }
    };
    tokio::select!{
      _ = announce_loop() => log::info!("announce loop exited: shutting down"),
      _ = tun_loop() => log::info!("tun loop exited: shutting down"),
      _ = link_loop() => log::info!("link loop exited: shutting down"),
      _ = tokio::signal::ctrl_c() => log::info!("got ctrl-c: shutting down")
    }
    log::info!("server exit");
}
