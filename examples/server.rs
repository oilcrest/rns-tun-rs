use std::time::Duration;

use env_logger;
use log;
use tokio;

use reticulum::destination::DestinationName;
use reticulum::destination::link::LinkEvent;
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    log::info!("server start");
    // tun adapter
    let mut adapter = match rns_tun::Adapter::new("10.0.0.1", 24) {
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
    /*
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
    */
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
    let mut out_link_events = transport.out_link_events();
    // run
    let mut run_loop = async || {
      let mut announce_interval = tokio::time::interval(Duration::from_secs(1));
      loop {
        tokio::select!{
          result = adapter.read() => {
            match result {
              Ok(bytes) => {
                println!("MSG: {bytes:x?}")
              }
              Err(err) => {
                log::error!("error reading TUN interface: {err:?}");
                break
              }
            }
          }
          link_event = out_link_events.recv() => {
            match link_event {
              Ok(link_event) => match link_event.event {
                LinkEvent::Data(payload) => {
                  log::trace!("link {} payload ({})", link_event.id, payload.len());
                  // TODO: handle payload
                }
                _ => {}
              }
              Err(err) => {
                log::error!("error receiving link events: {err:?}");
                break
              }
            }
          }
          _ = announce_interval.tick() => {
            log::trace!("sending announce");
            transport.send_announce(&in_destination, None).await
          }
        }
      }
    };
    tokio::select!{
      _ = run_loop() => log::info!("run loop exited: shutting down"),
      _ = tokio::signal::ctrl_c() => log::info!("got ctrl-c: shutting down")
    }
    log::info!("server exit");
}
