# zkvm-demo

A stand-alone browser demo of the [mpz](https://github.com/privacy-ethereum/mpz)
zk-vm: a prover and a verifier, both running as WebAssembly in your browser,
executing a Rust program (itself compiled to wasm) under zero-knowledge — and a
visualization of what each party does and doesn't learn.

**Status: spike.** The `rust/` crate proves the zk-vm stack
(`mpz-vm-ir` + `mpz-vm-zk`) compiles to `wasm32-unknown-unknown` and runs a
full Prover/Verifier execution of the `square` guest ((x+1)² over a private
input) inside headless Chrome. Single-threaded, no SharedArrayBuffer, no
special headers — it runs anywhere a wasm page loads.

## Layout

| Path | Purpose |
| --- | --- |
| `rust/` | wasm-bindgen wrapper around the mpz zk-vm (currently the spike: both parties in one instance over an in-memory duplex). |
| `web/` | (planned) Vite + TS UI: prover pane left, verifier pane right, each party in its own web worker with a tapped `MessageChannel` transport. |

## Design decisions (from the planning jam)

- **Self-serve, static, no backend.** Two web workers with separate wasm
  memories; the page relays (and can throttle) their messages, which yields
  the wire visualization and a slow-motion control for free.
- **Single-threaded wasm** for maximum compatibility (no COOP/COEP headers,
  works on GitHub Pages and phones). Demo-sized programs don't need rayon.
- **Ideal RCOT** ("trusted setup — demo" banner) for v1; real Ferret OT later.
- Guided stepper UX, a date-picker age check as the narrative anchor, and a
  "cheat" button showing a tampered proof being rejected.

## Running the spike

Requires a sibling checkout of mpz at `../mpz` (path dependencies; will become
pinned git deps once the demo stabilizes).

```sh
cd rust
cargo test                              # native: nothing yet (bindings are wasm-only)
wasm-pack test --headless --chrome      # the browser smoke test
```
