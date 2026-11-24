#!/bin/sh
# Build the engine for the web player: web/agb.wasm.
set -e
cd "$(dirname "$0")/.."
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release --target wasm32-unknown-unknown -p agb-web
cp target/wasm32-unknown-unknown/release/agb_web.wasm web/agb.wasm
ls -l web/agb.wasm
