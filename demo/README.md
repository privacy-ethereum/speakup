# Speakup demo

A stand-alone browser demo of
[Speakup](https://privacy-ethereum.github.io/speakup/), the zero-knowledge
virtual machine built on [mpz](https://github.com/privacy-ethereum/mpz): a
prover and a verifier, both running as WebAssembly in your browser, executing
a Rust program (itself compiled to wasm) under zero-knowledge — and a
visualization of what each party does and doesn't learn.

**Status: working v0 over the real protocol.** Eight guests run end-to-end in
the browser behind a two-pane prover/verifier UI — `square` ((x+1)² of a
private number), `age` (prove 18+ without revealing the birth date, with a
date picker), `sha256` (digest of a private message, up to 128 KB with size presets), and `regex` (prove a
**private** string matches a **public** pattern via an oblivious DFA — the
host compiles the regex with `regex-automata`, the guest evaluates the table
branch-free over a one-hot state vector; demo limits: 32 DFA states, 16 byte
classes, 256-byte strings), `luhn` (prove a private card number passes the
Luhn checksum),
`csv` (prove the average of one column of a **private CSV document** reaches
a public threshold — the guest parses the CSV *inside the VM*, branch-free:
oblivious column tracking, digit-by-digit number building, and validation,
revealing neither the contents, the row count, nor the sum),
`json` (**a claim about a private JSON document** — paste or edit any JSON,
e.g. the default bank statement, and either **assert** that the value at a
public path equals a public string ("my CHF balance is 50'000'000", revealing
only the 0/1 flag) or **disclose** that one value; the document is parsed
*outside* the VM into a public node table and the guest re-derives the full
JSON grammar from the private bytes branch-free, so the verifier learns the
structure and the claim but no other field), and
`transcript` (**a claim about a captured HTTPS exchange**: the
[transcript-verify](https://github.com/0xtsukino/tlsn-utils/tree/feat/transcript-verify/transcript-verify)
host parser turns the private request/response bytes into a public span
table *outside* the VM; the guest re-derives every claim from the private
bytes branch-free — HTTP grammar, header tiling, Content-Length framing, the
full JSON node tree — and then either **asserts** that the claimed field
equals a public expected string (revealing only the 0/1 flag: "the API
assigned my POST `id` = 101", with the request body hidden) or
**discloses** that one field, hiding every other header and value; the
claim can target a JSON value at a public path or a single **header line**
of either side — and with a header claim the response body is covered
opaquely, so non-JSON exchanges are provable too) —
with correlated randomness from the **real OT stack** (Chou-Orlandi base OT, KOS extension, Ferret expansion), not an ideal
functionality. **Each party runs in its own
web worker** — two isolated WebAssembly memories — speaking the mpz protocol
over a `MessageChannel`; the page relays the messages and surfaces live
traffic counters (a square proof completes in ~130 ms). The wasm is
multithreaded: each party runs a rayon pool on nested workers via
[web-spawn](https://github.com/tlsnotary/tlsn-utils), so the heavy proving
steps use all your cores. That requires cross-origin isolation
(SharedArrayBuffer); a tiny service worker shim provides it on GitHub Pages.

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
cd rust && wasm-pack build --release --target web --out-dir ../web/public/pkg -- --features no-bundler
cd ../web && npm install && npm run dev   # then open http://localhost:5173
```

## Design decisions (from the planning jam)

- **Self-serve, static, no backend.** Two web workers with separate wasm
  memories; the page relays (and can throttle) their messages, which yields
  the wire visualization and a slow-motion control for free.
- **Multithreaded wasm** (shared memory + rayon via web-spawn): the
  QuickSilver consistency check and the OT stack parallelize across cores,
  which is what makes the bigger sha-256 inputs interactive. The COOP/COEP
  headers this needs come from the dev server locally and from a
  `coi-serviceworker` shim on GitHub Pages.
- **Real OT from the start** (the ideal-RCOT pair shares memory, so it could
  never span separate workers anyway). Setup costs ~tens of ms in-browser.
- Guided stepper UX, a date-picker age check as the narrative anchor, and a
  "cheat" button showing a tampered proof being rejected.

## Feature flags

Three features are gated behind flags (default **off**) in
[`web/src/config.ts`](web/src/config.ts), pending a decision on whether they
belong in this demo. Try them without a rebuild via URL params:

| Flag | URL param | What it adds |
| --- | --- | --- |
| `slowMotion` | `?slow=1` | Relay-delay slider and a step-through-messages mode (the page relays all protocol traffic, so it can pause it). |
| `cheat` | `?cheat=1` | "Tamper with a message" button: the relay flips one bit in protocol message #10 and the verifier rejects the proof. |
| `watEditor` | `?wat=1` | "custom (wat)" tab: write a guest in WebAssembly text format; both parties compile the same (public) source and run it over a private `x`. |

## Developing

Requires [wasm-pack](https://rustwasm.github.io/wasm-pack/); rustup picks up
the pinned nightly toolchain (shared-memory wasm needs `build-std`) from
`rust/rust-toolchain.toml` automatically.

```sh
cd guests && cargo test                 # native unit tests of the guest programs
cd rust && wasm-pack test --headless --chrome --release -- --features no-bundler   # end-to-end browser tests
```

To customize a guest, edit it under `guests/` and rebuild the pkg — the
bindings' build script recompiles the guest wasm automatically:

```sh
cd rust && wasm-pack build --release --target web --out-dir ../web/src/pkg
```
