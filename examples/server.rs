use std::sync::Arc;

use env_logger;
use log;

use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    log::info!("server start");
    // tun adapter
    let adapter = match rns_tun::Adapter::new("10.88.0.1", 24) {
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
    // start reticulum
    let id = PrivateIdentity::new_from_name("server");
    let transport = Transport::new(TransportConfig::new("server", &id, true));
    let _ = transport.iface_manager().lock().await.spawn(
        TcpServer::new("0.0.0.0:4242", transport.iface_manager()),
        TcpServer::spawn,
    );
    let destination = SingleInputDestination::new(id, DestinationName::new("example", "server"));
    let dest = Arc::new(tokio::sync::Mutex::new(destination));
    // run
    let run_loop = async || loop {
        transport.send_announce(&dest, None).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    };
    tokio::select!{
      _ = run_loop() => {}
      _ = tokio::signal::ctrl_c() => log::info!("shutting down")
    }
    log::info!("server exit");
}
