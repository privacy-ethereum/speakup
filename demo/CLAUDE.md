# Speakup demo — working notes

Browser demo of Speakup (the mpz zk-vm): prover and verifier in separate web
workers, real OT stack, eight example guests. See README.md for the
user-facing picture; this file is for working on the code.

## Build & test

```sh
# guests: native unit tests (mpz-vm-sys is a no-op off the VM)
cd guests && cargo test

# bindings: e2e browser tests (headless Chrome) — the real test suite
cd rust && wasm-pack test --headless --chrome --release -- --features no-bundler

# rebuild the web pkg after ANY rust/ or guests/ change
cd rust && wasm-pack build --release --target web --out-dir ../web/public/pkg -- --features no-bundler

# web app
cd web && npm run dev          # --host needs https: threads want SharedArrayBuffer
cd web && ./node_modules/.bin/tsc --noEmit   # type check (vite doesn't)
```

`rust/build.rs` compiles `guests/` to wasm32 automatically (artifacts land in
OUT_DIR; `lib.rs` embeds them) — editing a guest and rebuilding the pkg is one
step. mpz comes from git, branch `v2`, fetched with the git CLI (libgit2
chokes on the repo); `Cargo.lock` pins the exact rev (≥ c215974 needed: the
two-pass QuickSilver is what makes sha-256 64 KB fit in memory at all).
Cargo only honors `[patch.crates-io]` in the top-level workspace, so mpz's
own patches do NOT reach this build — rust/Cargo.toml mirrors them (the
wasm-simd `aes` fork: fixsliced AES over v128, ~10–25% off every run; the
OT stack sits on it). Re-check mpz's patch section on every pin bump.

## Threads

The bindings are shared-memory wasm: pinned nightly + build-std
(`rust/rust-toolchain.toml`), atomics + memory link flags
(`rust/.cargo/config.toml`). Each party worker calls the `initialize(threads)`
export, which starts web-spawn's spawner and a rayon global pool on nested
workers — the QuickSilver check and the OT stack are the rayon users
(hardwareConcurrency/2 per party). Hard-won constraints:

- Guests must stay atomics-free. Cargo config discovery is cwd-based, so
  build.rs runs the guest cargo from `../guests` — without that, the
  shared-memory rustflags leak into the guest build and the zk-vm rejects it.
- rayon sections block the calling thread via `Atomics.wait` — legal in a
  worker, fatal on the main browser thread. That's why the e2e tests run
  `run_in_dedicated_worker` and call `initialize` first.
- The pkg is NOT bundled (built with web-spawn's `no-bundler` glue into
  `web/public/pkg`, dynamically imported by `party.worker.ts`): vite inlines
  web-spawn's nested thread-worker as a `data:` URL, whose `import.meta.url`
  resolves nothing — bundled builds break only in production.
- Cross-origin isolation: vite dev/preview send COOP/COEP headers
  (`vite.config.ts`); GitHub Pages can't set headers, so `index.html` loads
  `coi-serviceworker` (npm dep, served/copied by a tiny vite plugin; costs
  one reload on first visit).
- `--max-memory=4294967296` (the wasm32 ceiling): shared memories must
  declare a max, and sha-256 64 KB peaks past 2 GB. 128 KB still aborts
  (`RuntimeError: unreachable` = OOM panic) — that one needs upstream memory
  work, not a flag.

## The one rule for guest code

Guests run on a zk-vm that CANNOT branch on, index by, shift by, or `select`
on private (symbolic) data. Source-level branch-freedom is NOT enough — LLVM
re-introduces forbidden ops. Known lowering traps, all hit in practice:

1. byte-array comparisons become early-exit `bcmp` (a private branch) —
   pin the XOR-accumulator with `black_box` before comparing it to zero;
2. `flag & value` of two 0/1 flags becomes `select` — pin both operands;
3. short-circuitable arithmetic gets hoisted behind a private branch —
   pin intermediate flags;
4. `0 - flag` (the mask idiom) becomes `select(flag, -1, 0)` when LLVM can
   prove `flag ∈ {0,1}` — pin EVERY flag at creation (`black_box((a == b) as
   i32)`), not just the computed masks. The VM rejects `select` even with a
   concrete condition when an arm is symbolic.

When a run fails with `Unsupported("zk-vm: op not yet supported: Select…")`,
disassemble and find it: `wasm-tools print guests/target/wasm32-unknown-unknown/release/<g>.wasm | grep -n select`
(an all-concrete `select` from `len.min(CAP)` is fine and expected).

Public data is concrete in the VM: branching and indexing on public params,
public writes (`Write::Public`), and public loop bounds are all fine — the
regex table and CSV column index rely on this.

## Architecture notes

- One worker per party (`web/src/party.worker.ts`, spawned twice), each with
  its own wasm memory, talking over a MessageChannel. The page relays every
  message (`web/src/main.ts`): that's where traffic counters, the slow-motion
  queue, and the cheat tamper live.
- `rust/src/port_io.rs` adapts a MessagePort to AsyncRead/AsyncWrite via mpsc
  pumps (the mux needs Send+Sync; MessagePort is neither). `port_mux.rs` runs
  a `tlsn-mux` connection over it and implements mpz's `Mux`, so each context
  fork gets its own logical stream (`Context::new` instead of
  `new_single_threaded`).
- Every program has `prover_*`/`verifier_*` entry points (used by the app)
  plus a single-instance `*_zkvm` (used by tests), sharing per-role inners.
- "view full source" modal: the guest `lib.rs` files are `?raw`-imported
  from `../guests` (vite `server.fs.allow` covers it). The wasm-info line
  under each program box is computed per party: a `guest_info` worker
  request hashes that worker's embedded module (`guest_wasm` export), so
  the two panes show independently computed, matching hashes.
- Guest crates must NEVER be linked into `rust/` (mpz-vm-sys emits `vc.*`
  wasm imports nothing satisfies). Shared logic goes in a separate crate with
  no mpz-vm-sys dep — see `guests/regex-core` and `guests/transcript-core`.
- The transcript guest is the advice pattern from tlsn-utils'
  transcript-verify PR (0xtsukino fork, rev pinned in both Cargo.tomls),
  inverted for a VM that can't branch on private data: `parse_transcript`
  runs host-side (prover worker, off the VM), `transcript_core::encode`
  flattens the span table + claims (method/target/Host/status/JSON path)
  into a u32 word table written as PUBLIC guest data, and the guest
  re-derives everything from the private bytes branch-free — the public
  table drives all control flow. Two claim modes (`W_CLAIM_MODE`): assert
  ("the value at the path equals this public string", 0/1 only — the UI
  default; a wrong expected value encodes fine and the proof honestly
  fails) and disclose (`reveal_bytes` of an OUT buffer the guest masks to
  zeros unless the symbolic ok flag holds — reveals can't be skipped on a
  private condition, but CAN be skipped on the public mode). Request
  bodies are covered as opaque private bytes (framing pinned via the CL
  digits, contents unconstrained). Table v2 adds claim TARGETS
  (`W_CLAIM_TARGET`): besides the JSON body value, the claim can name one
  header line (`req:<i>`/`resp:<i>` claim strings from the UI; index +
  lowercase name public, name pinned case-insensitively, value through
  the same assert/disclose machinery — `Disclosure::value_in_sent` tells
  the guest which buffer to mask from). With a header claim the JSON
  section must be empty and the response body is covered opaquely like
  request bodies — non-JSON exchanges (HTML etc.) become provable. Unused
  claim words are pinned to 0 in either mode (no-slack property). `transcript-core` holds the layout
  consts, the verifier (`verify`), the encoder (which self-checks every
  encoding), and the tests — including exhaustive byte-flip differentials
  against the real upstream `validate` (for request bodies: against the
  legal opaque-claim table). Scope cuts (all strictly stricter than
  upstream): Content-Length framing only, ASCII JSON strings, raw dup-key
  compare. The fixture (jsonplaceholder_post: hidden JSON POST body, 201
  response with the assigned id) is copied by rust/build.rs out of the
  dependency's git checkout (located via `cargo metadata`) — no
  third-party bytes vendored into the repo. Gotcha: a guest-side `match`
  merging Ok/Err of `verify` lowers to `select`s with the SYMBOLIC ok flag
  as an arm — early-return on Err instead (concrete path, both parties
  take it identically). Every table word is pinned (unused words must be
  0) so the tampered-word tests stay exhaustive.
- The json guest (`guests/json`) is the transcript pattern minus HTTP: the
  private input is one user-editable JSON document; the flat table
  (`transcript_core::json`, its own compact header — nodes + path + claim
  only) drives the same shared walk (`verify_json_claim`, extracted from
  the transcript verifier — both formats accept the identical byte-level
  JSON language, and the transcript differential tests cover the shared
  code). Host-side parsing of arbitrary documents reuses the upstream
  parser via `json::synth_exchange`, a minimal CL-framed wrapper that
  never enters the VM (upstream emits JSON nodes only for
  `application/json` bodies, hence the synthetic Content-Type). The
  default document in web/index.html mirrors tlsn-extension's swissbank
  demo fixture (servers/swissbank/src/data/swissbankdata.json — the data
  behind swissbank.plugin.ts; fake demo data, so inlining it is fine);
  default claim `accounts.CHF`, default mode disclose, matching the
  plugin's reveal handlers. Unlike the transcript tab the textarea is
  editable: every edit re-requests `json_info` (paths) and `json_public`
  (words) from the prover worker, with the doc echoed back in each
  response so stale answers are dropped.
- Feature flags: `web/src/config.ts`, URL override `?cheat=1` — tamper
  button (default off, undecided whether it ships). The WAT editor was
  replaced by the "custom wasm" tab (default on): drop a compiled guest,
  the prover worker inspects it (`module_exports`), the page builds one
  input per argument with a private/public toggle, and the generic
  `prover_custom`/`verifier_custom` entry points call any exported
  function over i32/i64 scalars. The verifier request carries zeroed
  private values. NOTE: never name a `#[wasm_bindgen]` parameter `wasm` —
  the generated glue shadows its own `wasm` instance object.
  The relay-delay slider is always shown; the step-through-messages mode
  was dropped. The Run button morphs into Abort during a run (abort
  terminates and respawns both workers — the protocol can't be interrupted
  any other way).

## Status / open items

- Repo: github.com/tlsnotary/speakup-demo (public).
- GitHub Pages: `.github/workflows/deploy.yml` builds the wasm pkg + vite
  site on every push to main and deploys via actions/deploy-pages. Repo
  settings → Pages → source must be "GitHub Actions". Vite base is
  `/speakup-demo/`.
- Perf anchors (Chrome, M-series 18 cores → 9 threads/party, threaded build
  + mpz c215974 + wasm-simd aes patch): square ≈ 0.08 s; regex email ≈ 0.28 s;
  csv 4 rows ≈ 0.11 s; transcript (jsonplaceholder_post, 1543 private bytes)
  ≈ 0.37 s / 270 msgs / ~1.5 MB; sha-256 16 KB ≈ 3.0 s / 400 msgs / ~2.5 MB;
  64 KB ≈ 11 s (OOM-aborted before c215974); 128 KB still OOMs. Pre-aes-patch:
  square 0.10 s, regex 0.31 s, transcript 0.42 s, sha-256 16 KB 3.7 s, 64 KB
  13 s. Pre-threads baselines: square 0.4 s, sha-256 16 KB 10.7 s, regex 1.6 s.
- `ecdsa` is a placeholder tab only (web/src/main.ts PROGRAMS entry + the
  index.html button with the `.soon` badge): no guest crate or bindings
  exist; `requests()` returns a coming-soon message so Run never starts a
  protocol. Implementing it means the usual full set: guest crate,
  rust/ entry points + build.rs line, worker request type, PROGRAMS entry.
- Possible next: the ECDSA guest (signature verification inside the VM),
  guided stepper narrative, more polish; Speakup RAM support will
  eventually allow private-offset designs (see mpz age-span discussion)
  and may relax the black_box discipline if `select` lands.
