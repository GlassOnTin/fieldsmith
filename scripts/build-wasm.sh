#!/usr/bin/env bash
# Build the Fieldsmith WebAssembly package into web/src/pkg.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --target wasm32-unknown-unknown -p fieldsmith-wasm --release
wasm-bindgen target/wasm32-unknown-unknown/release/fieldsmith_wasm.wasm \
  --out-dir web/src/pkg --target web
echo "wasm: $(du -h web/src/pkg/fieldsmith_wasm_bg.wasm | cut -f1)"
