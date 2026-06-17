#!/usr/bin/env bash
# Builds the speakup-wasm bindings crate to a wasm-bindgen pkg and publishes it
# to web/public/pkg (served as-is, not bundled) with web-spawn's no-bundler
# glue — see demo/web/src/party.worker.ts. Runs from speakup-wasm so its
# .cargo/config.toml (shared-memory RUSTFLAGS) and rust-toolchain.toml apply.
# Incremental: no --clean, so cargo alone decides what needs recompiling.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/speakup-wasm"

wasm-pack build --release --target web --out-dir ../web/public/pkg -- --features no-bundler
echo "built bindings -> web/public/pkg"
