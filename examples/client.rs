use std::sync::Arc;
use env_logger;
use log;

use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    log::info!("client start");
    // tun adapter
    let adapter = match rns_tun::Adapter::new("10.88.0.2", 24) {
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
    let transport = Transport::new(TransportConfig::default());
    let _ = transport.iface_manager().lock().await
        .spawn(TcpClient::new("127.0.0.1:4242"), TcpClient::spawn);
    let id = PrivateIdentity::new_from_name("client");
    let destination = SingleInputDestination::new(id, DestinationName::new("example", "app"));
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

    log::info!("client exit");
}
