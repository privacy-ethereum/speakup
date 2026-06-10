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
Ferret expansion), not an ideal functionality. Both parties currently execute
in one web worker over an in-memory duplex (clearly labeled in the UI);
splitting them into one worker per party over a `MessageChannel` transport is
the next milestone. Single-threaded, no SharedArrayBuffer, no special
headers — it runs anywhere a wasm page loads.

## Layout

| Path | Purpose |
| --- | --- |
| `rust/` | wasm-bindgen wrapper around the mpz zk-vm (both parties in one instance over an in-memory duplex, for now). |
| `web/` | Vite + TS UI: prover pane left, verifier pane right, the wasm in a web worker. |

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

## Running the spike

Requires a sibling checkout of mpz at `../mpz` (path dependencies; will become
pinned git deps once the demo stabilizes).

```sh
cd rust
cargo test                              # native: nothing yet (bindings are wasm-only)
wasm-pack test --headless --chrome      # the browser smoke test
```
