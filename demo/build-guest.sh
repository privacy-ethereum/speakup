#!/usr/bin/env bash
# Builds the guest programs to wasm and publishes them as static assets for the
# web app (loaded at runtime by the orchestration JS) and, for sha256, the
# bindings' browser test. The bindings crate (speakup-wasm) contains no guest
# code, so guest compilation lives here instead of in a build.rs.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/guests"

# Clean env so the bindings' shared-memory RUSTFLAGS / atomics link flags
# (speakup-wasm/.cargo/config.toml) cannot leak into the guest build — the
# zk-vm rejects atomics. Running from ../guests already keeps that config out
# of cargo's discovery path; scrubbing the env covers inherited flags too.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR \
  cargo build --release --target wasm32-unknown-unknown -p sha256-guest -p sudoku-guest

rel="$here/guests/target/wasm32-unknown-unknown/release"
mkdir -p "$here/web/public/guests"
cp "$rel/sha256_guest.wasm" "$here/web/public/guests/sha256.wasm"
cp "$rel/sudoku_guest.wasm" "$here/web/public/guests/sudoku.wasm"
cp "$rel/sha256_guest.wasm" "$here/speakup-wasm/tests/sha256.wasm"
echo "built guests -> web/public/guests/{sha256,sudoku}.wasm"
