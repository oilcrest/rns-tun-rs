use log;
use riptun::TokioTun;

// TODO: config?
const MTU: usize = 1500;

pub struct Adapter {
  tun: TokioTun,
  read_buf: tokio::sync::Mutex<[u8; MTU]>
}

impl Adapter {
  pub fn new(ip: &str, network_mask: u8) -> Result<Self, riptun::Error> {
    const NQUEUES : usize = 1;
    log::debug!("creating tun device");
    let tun = TokioTun::new ("rip%d", NQUEUES)?;
    log::debug!("created tun device: {}", tun.name());
    log::debug!("adding ip addr: {}/{}", ip, network_mask);
    let output = std::process::Command::new("ip")
      .arg("addr")
      .arg("add")
      .arg(&format!("{ip}/{network_mask}"))
      .arg("brd")
      .arg(ip)
      .arg("dev")
      .arg(tun.name())
      .output()
      .map_err(riptun::Error::from)?;
    if !output.status.success() {
      return Err(std::io::Error::other(format!("ip addr add command failed ({:?})",
        output.status.code())).into());
    }
    log::debug!("{} setting link up", tun.name());
    let output = std::process::Command::new("ip")
      .arg("link")
      .arg("set")
      .arg("dev")
      .arg(tun.name())
      .arg("up")
      .output()
      .map_err(riptun::Error::from)?;
    if !output.status.success() {
      return Err(std::io::Error::other(format!("ip link set command failed ({:?})",
        output.status.code())).into());
    }
    let adapter = Adapter {
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
