#!/usr/bin/env bash

cargo build --bin client && sudo ./target/debug/client -s 192.168.1.131:4242

exit 0
