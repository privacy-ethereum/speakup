# zkvm-demo

A stand-alone browser demo of the [mpz](https://github.com/privacy-ethereum/mpz)
zk-vm: a prover and a verifier, both running as WebAssembly in your browser,
executing a Rust program (itself compiled to wasm) under zero-knowledge — and a
visualization of what each party does and doesn't learn.

**Status: working v0 over the real protocol.** Three guests run end-to-end in
the browser behind a two-pane prover/verifier UI — `square` ((x+1)² of a
private number), `age` (prove 18+ without revealing the birth date, with a
date picker), and `sha256` (digest of a private message) — with correlated
randomness from the **real OT stack** (Chou-Orlandi base OT, KOS extension,
Ferret expansion), not an ideal functionality. **Each party runs in its own
web worker** — two isolated WebAssembly memories — speaking the mpz protocol
over a `MessageChannel`; the page relays the messages and surfaces live
traffic counters (a square proof is ~21 messages / ~900 KB, ~330 ms).
Single-threaded, no SharedArrayBuffer, no special headers — it runs anywhere
a wasm page loads.

## Layout

| Path | Purpose |
| --- | --- |
| `guests/` | The example programs that run *on* the zk-vm (square, age, sha256) — edit these to customize the demo; the build picks changes up automatically. |
| `rust/` | wasm-bindgen wrapper around the mpz zk-vm: per-party entry points over a `MessagePort` duplex (`port_io.rs`), plus single-instance variants for tests. Its `build.rs` compiles `guests/` to wasm. |
| `web/` | Vite + TS UI: prover pane left, verifier pane right, one worker per party, the page relaying and counting protocol traffic. |

The repo is self-contained: mpz is consumed as a git dependency pinned to its
[`v2` branch](https://github.com/privacy-ethereum/mpz/tree/v2), and the guest
wasm is built from the in-repo sources.

## Try it

```sh
cd rust && wasm-pack build --release --target web --out-dir ../web/src/pkg
cd ../web && npm install && npm run dev   # then open http://localhost:5173
```

## Design decisions (from the planning jam)

- **Self-serve, static, no backend.** Two web workers with separate wasm
  memories; the page relays (and can throttle) their messages, which yields
  the wire visualization and a slow-motion control for free.
- **Single-threaded wasm** for maximum compatibility (no COOP/COEP headers,
  works on GitHub Pages and phones). Demo-sized programs don't need rayon.
- **Real OT from the start** (the ideal-RCOT pair shares memory, so it could
  never span separate workers anyway). Setup costs ~tens of ms in-browser.
- Guided stepper UX, a date-picker age check as the narrative anchor, and a
  "cheat" button showing a tampered proof being rejected.

## Developing

Requires the `wasm32-unknown-unknown` target and [wasm-pack](https://rustwasm.github.io/wasm-pack/).

```sh
cd guests && cargo test                 # native unit tests of the guest programs
cd rust && wasm-pack test --headless --chrome   # end-to-end browser tests
```

To customize a guest, edit it under `guests/` and rebuild the pkg — the
bindings' build script recompiles the guest wasm automatically:

```sh
cd rust && wasm-pack build --release --target web --out-dir ../web/src/pkg
```
