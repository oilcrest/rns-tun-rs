# `rns-tun`

> Reticulum TUN adapter

## Building and running

Setting up the TUN adapter requires root permissions:
```
cargo build
sudo ./target/debug/server -p 4242
sudo ./target/debug/client -s 192.168.0.99:4242
```
