use std::sync::Arc;
use env_logger;
use log;

use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::destination::link::LinkEvent;
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::transport::{Transport, TransportConfig};
use rns_tun::*;

const CONFIG_PATH: &str = "Client.toml";

#[tokio::main]
async fn main() {
  let config: rns_tun::ClientConfig = {
    use std::io::Read;
    let mut s = String::new();
    let mut f = std::fs::File::open(CONFIG_PATH).unwrap();
    assert!(f.read_to_string(&mut s).unwrap() > 0);
    toml::from_str(&s).unwrap()
  };
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
    .init();
  log::info!("client start");
  // tun adapter
  let client = match rns_tun::Client::new(config) {
    Ok(client) => client,
    Err(err) => match err {
      CreateAdapterError::RiptunError(riptun::Error::Unix {
        source: nix::errno::Errno::EPERM
      }) => {
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
  let id = PrivateIdentity::new_from_name("client");
  let transport = Transport::new(TransportConfig::new("client", &id, true));
  let _ = transport.iface_manager().lock().await
      .spawn(TcpClient::new("192.168.1.131:4242"), TcpClient::spawn);
  let destination = SingleInputDestination::new(id, DestinationName::new("example", "client"));
  let dest = Arc::new(tokio::sync::Mutex::new(destination));
  // run
  client.run(transport).await;
  log::info!("client exit");
}
