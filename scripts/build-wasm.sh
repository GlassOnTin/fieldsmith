#!/usr/bin/env bash
# Build the Fieldsmith WebAssembly packages:
#   web/src/pkg     single thread (stable toolchain) — loaded when the page is not
#                   cross-origin isolated
#   web/src/pkg-mt  threaded: shared memory + a rayon pool on Web Workers
#                   (wasm-bindgen-rayon). Needs nightly with rust-src for -Z build-std,
#                   and COOP/COEP headers at runtime (web/vite.config.ts, README).
# FIELDSMITH_WASM_ST_ONLY=1 skips the threaded build.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --target wasm32-unknown-unknown -p fieldsmith-wasm --release
wasm-bindgen target/wasm32-unknown-unknown/release/fieldsmith_wasm.wasm \
  --out-dir web/src/pkg --target web
echo "wasm (single thread): $(du -h web/src/pkg/fieldsmith_wasm_bg.wasm | cut -f1)"

if [[ "${FIELDSMITH_WASM_ST_ONLY:-}" == 1 ]]; then exit 0; fi

# Shared, imported memory with a 4 GiB maximum (the wasm32 ceiling); the TLS exports are
# what wasm-bindgen's thread start-up needs. Newer rustc no longer implies these linker
# flags from +atomics, so they are spelled out.
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory \
-C link-arg=--shared-memory -C link-arg=--import-memory \
-C link-arg=--max-memory=4294967296 \
-C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size \
-C link-arg=--export=__tls_align -C link-arg=--export=__tls_base" \
  cargo +nightly build --target wasm32-unknown-unknown -Z build-std=panic_abort,std \
  --release -p fieldsmith-wasm --features threads --target-dir target/wasm-mt
wasm-bindgen target/wasm-mt/wasm32-unknown-unknown/release/fieldsmith_wasm.wasm \
  --out-dir web/src/pkg-mt --target web
# The pool helper imports its package as a directory ('../../..'), which only resolves
# with a wasm-pack package.json; point it at the module so Vite/Rollup can bundle it.
sed -i "s|import('../../..')|import('../../../fieldsmith_wasm.js')|" \
  web/src/pkg-mt/snippets/wasm-bindgen-rayon-*/src/workerHelpers.js
grep -q "import('../../../fieldsmith_wasm.js')" \
  web/src/pkg-mt/snippets/wasm-bindgen-rayon-*/src/workerHelpers.js \
  || { echo "workerHelpers.js import rewrite failed" >&2; exit 1; }
echo "wasm (threaded): $(du -h web/src/pkg-mt/fieldsmith_wasm_bg.wasm | cut -f1)"
