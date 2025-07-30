#!/usr/bin/env bash

cargo build --bin server && sudo ./target/debug/server -p 4242

exit 0
