use std::sync::Arc;
use env_logger;
use log;

use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::destination::link::LinkEvent;
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    log::info!("client start");
    // tun adapter
    let mut adapter = match rns_tun::Adapter::new("10.0.0.2", 24) {
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
    // configure forwarding to tun for example IP 104.16.184.241
    // TODO: command-line arg
    let example_ip = "104.16.184.241/32";
    log::info!("adding route for {example_ip}");
    let output = match std::process::Command::new("ip")
      .arg("route")
      .arg("add")
      .arg(example_ip)
      .arg("dev")
      .arg(adapter.tun().name())
      .output()
    {
      Ok(output) => output,
      Err(err) => {
        log::error!("error adding route for {example_ip}: {err:?}");
        std::process::exit(1)
      }
    };
    if !output.status.success() {
      log::error!("ip route add command failed ({:?})", output.status.code());
      std::process::exit(1)
    }
    // start reticulum
    let id = PrivateIdentity::new_from_name("client");
    let transport = Transport::new(TransportConfig::new("client", &id, true));
    let _ = transport.iface_manager().lock().await
        .spawn(TcpClient::new("192.168.1.131:4242"), TcpClient::spawn);
    let destination = SingleInputDestination::new(id, DestinationName::new("example", "client"));
    let dest = Arc::new(tokio::sync::Mutex::new(destination));
    // announce
    /*
    let announce_loop = async || loop {
        log::trace!("sending announce");
        transport.send_announce(&dest, None).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    };
    */
    // set up links
    let mut announce_recv = transport.recv_announces().await;
    let mut add_links = async || while let Ok(announce) = announce_recv.recv().await {
        let destination = announce.destination.lock().await;
        let _link = transport.link(destination.desc).await;
    };
    // listen to tun and forward to links
    let mut run_loop = async || while let Ok(bytes) = adapter.read().await {
        log::trace!("got tun bytes ({})", bytes.len());
        transport.send_to_all_out_links(bytes.as_slice()).await;
    };
    // TODO: handle incoming link messages
    let mut link_loop = async || {
      let mut out_link_events = transport.out_link_events();
      while let Ok(link_event) = out_link_events.recv().await {
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
          _ => {}
        }
      }
    };
    // run
    tokio::select!{
      _ = run_loop() => log::info!("run loop exited: shutting down"),
      _ = add_links() => log::info!("add links exited: shutting down"),
      _ = link_loop() => log::info!("link loop exited: shutting down"),
      //_ = announce_loop() => log::info!("announce loop exited: shutting down"),
      _ = tokio::signal::ctrl_c() => log::info!("got ctrl-c: shutting down")
    }

    log::info!("client exit");
}
