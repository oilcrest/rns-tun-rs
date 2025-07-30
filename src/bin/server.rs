//! Serves reticulum TCP interface

use clap::Parser;
use env_logger;
use log;
use reticulum::identity::PrivateIdentity;
use reticulum::iface::tcp_server::TcpServer;
use reticulum::transport::{Transport, TransportConfig};
use riptun;
use tokio;

use rns_tun;

const CONFIG_PATH: &str = "Server.toml";

/// Command line arguments
#[derive(Parser)]
#[clap(name = "Rns Tun Server", version)]
pub struct Command {
  #[clap(short = 'p', help = "TCP listen port number")]
  pub port: u16
}

#[tokio::main]
async fn main() {
  // parse command line args
  let cmd = Command::parse();
  // load config
  let config: rns_tun::ServerConfig = {
    use std::io::Read;
    let mut s = String::new();
    let mut f = std::fs::File::open(CONFIG_PATH).unwrap();
    assert!(f.read_to_string(&mut s).unwrap() > 0);
    toml::from_str(&s).unwrap()
  };
  // init logging
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
    .init();
  log::info!("server start with port {}", cmd.port);
  // tun adapter
  let server = match rns_tun::Server::new(config) {
    Ok(adapter) => adapter,
    Err(err) => match err {
      rns_tun::CreateAdapterError::RiptunError(riptun::Error::Unix {
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
  log::info!("starting reticulum");
  let id = PrivateIdentity::new_from_name("rns-tun-server");
  let transport = Transport::new(TransportConfig::new("server", &id, true));
  let _ = transport.iface_manager().lock().await.spawn(
    TcpServer::new(format!("0.0.0.0:{}", cmd.port), transport.iface_manager()),
    TcpServer::spawn,
  );
  // run
  server.run(transport, id).await;
  log::info!("server exit");
}
