//! Connects to server node via reticulum TCP interface.

use clap::Parser;
use env_logger;
use log;
use reticulum::destination::{DestinationName, SingleInputDestination};
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_client::TcpClient;
use reticulum::transport::{Transport, TransportConfig};

use rns_tun::*;

const CONFIG_PATH: &str = "Client.toml";

/// Command line arguments
#[derive(Parser)]
#[clap(name = "Rns Tun Server", version)]
pub struct Command {
  #[clap(short = 's', help = "Server node IP address:port")]
  pub server: String
}

#[tokio::main]
async fn main() {
  // parse command line args
  let cmd = Command::parse();
  // load config
  let config: rns_tun::ClientConfig = {
    use std::io::Read;
    let mut s = String::new();
    let mut f = std::fs::File::open(CONFIG_PATH).unwrap();
    assert!(f.read_to_string(&mut s).unwrap() > 0);
    toml::from_str(&s).unwrap()
  };
  // init logging
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
    .init();
  log::info!("client start with upstream server {}", cmd.server);
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
  let id = PrivateIdentity::new_from_name("rns-tun-client");
  let transport = Transport::new(TransportConfig::new("client", &id, true));
  let _ = transport.iface_manager().lock().await
    .spawn(TcpClient::new(cmd.server), TcpClient::spawn);
  let destination = SingleInputDestination::new(id, DestinationName::new("rns_tun", "client"));
  log::info!("created destination: {}", destination.desc.address_hash);
  // run
  client.run(transport).await;
  log::info!("client exit");
}
