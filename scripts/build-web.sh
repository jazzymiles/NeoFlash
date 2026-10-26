#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add wasm32-unknown-unknown
if ! command -v wasm-bindgen >/dev/null || [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.129" ]]; then
  cargo install wasm-bindgen-cli --version 0.2.129 --locked
fi
cargo build --locked --release -p neoflash-web --target wasm32-unknown-unknown
wasm-bindgen --target no-modules --no-modules-global NeoFlashWasm --out-name neoflash_web --out-dir target/web-runtime target/wasm32-unknown-unknown/release/neoflash_web.wasm
