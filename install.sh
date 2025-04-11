#!/usr/bin/env sh
cargo build --release
sudo cp target/release/congregation /usr/bin
