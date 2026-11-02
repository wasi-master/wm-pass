#!/bin/sh
# Builds the session for the browser into ui/src/wasm. wasm-bindgen must match Cargo.lock's version.
set -e
cd "$(dirname "$0")/.."
cargo build -p wmpass-wasm --target wasm32-unknown-unknown --profile wasm
wasm-bindgen target/wasm32-unknown-unknown/wasm/wmpass_wasm.wasm --out-dir ui/src/wasm --target web
