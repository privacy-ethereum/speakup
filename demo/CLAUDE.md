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
- Pkg cache busting: the pkg's stable-named files (public/ assets aren't
  hashed) + Pages' max-age=600 means a warm browser can pair new page code
  with the PREVIOUS deploy's glue/wasm ("x is not a function" right after
  a deploy). vite.config bakes a pkg content hash into `__PKG_VERSION__`;
  the worker appends `?v=` to the glue import AND passes the versioned
  wasm URL to `pkg.default({module_or_path})` (the glue's own fallback
  resolves relative to import.meta.url, dropping the query). Init errors
  post `error` before `ready`; the page shows pre-run errors instead of
  swallowing them.
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

## The other rule for guest code: memory authentication

The VM authenticates linear memory per byte: a load faults
(`MemAuthMissing { addr }`) unless that byte was either written by the host
(`Vm::write` before the call) or stored by a prior guest instruction in the
same run. There is **no bulk-memory support** — `memory.fill` and
`memory.copy` write bytes the VM never authenticates. The traps:

- A large local array **returned or moved** by value (a `[u64; N]` out of a
  helper, `core::array::from_fn` materializing then copying to its slot)
  lowers to `memory.copy`; reading it back faults. `memory.fill` of a local
  that is then **fully overwritten** is fine (sha-256's `block`/`w`) — only
  the unauthenticated `copy` (and a `fill`'d byte that's never re-stored) bite.
- So a guest must NOT slurp its big inputs into stack arrays. Read them
  straight from the authenticated static buffers the host wrote: pass the
  shared logic byte/word **accessors**, not `&[u8]`/`&[u64]`. The ecdsa guest
  does this (`verify(msg_len, |i| MSG[i], |i| word(&TABLE,i), …)`); a
  slice-backed `verify_slices` serves host code and native tests, where stack
  arrays are free. Confirm a clean build with
  `wasm-tools print … | grep -nE 'memory\.(copy|fill)'` — the only survivors
  should be tiny fully-overwritten locals and dynamic-size copies inside
  dlmalloc/fmt (concrete heap data, never read back as VM inputs).

## Architecture notes

- One worker per party (`web/src/party.worker.ts`, spawned twice), each with
  its own wasm memory, talking over a MessageChannel. The page relays every
  message (`web/src/main.ts`): that's where traffic counters, the simulated
  latency, and the cheat tamper live. Latency is per direction, overlapping
  (delivery scheduled at arrival + latency), so it costs latency × round-trip
  depth, not × message count. The tamper flips a middle byte of EVERY
  payload-carrying (≥ TAMPER_MIN_BYTES) prover→verifier message, not a single
  one: the relay numbers messages as they arrive interleaved across both
  directions, so a fixed index lands on different protocol data run to run,
  and a lone corrupted OT correlation can fall OUTSIDE the consistency-check
  sample — the proof then verifies anyway (the one outcome a tamper demo must
  never show; this was the actual bug when it was a single fixed-index flip,
  reproducible on the transcript/json tabs). Corrupting the whole
  prover→verifier stream is caught deterministically — the COT bootstrap
  check fires (verified on all eight example programs + ecdsa). Sub-256 B
  messages are skipped: a flipped frame header trips the transport ("frame
  size too big") not the crypto, and keeping the story about the proof.
- `rust/src/port_io.rs` adapts a MessagePort to AsyncRead/AsyncWrite via mpsc
  pumps (the mux needs Send+Sync; MessagePort is neither). Writes are
  buffered until `poll_flush`, so one mux flush batch = one postMessage —
  without this every frame costs two messages (header + body) and the
  page-visible msg counter doubles. `port_mux.rs` runs
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
- The ecdsa guest (`guests/ecdsa` + `guests/ecdsa-core`) proves REAL ECDSA
  verification (sha-256, `u₁·G + u₂·Q`, `x ≟ r`) over a private message AND a
  private signature; only the 0/1 verdict is revealed. The curve is a toy
  64-bit sibling of secp256k1 (`y² = x³ + 2` over `p = 2⁶⁴ − 453`, prime
  group order `n = 2⁶⁴ − 7386677115`, cofactor 1) — no security, sized to fit
  the VM's proving budget; P-256 width needs ~100× the i64 multiplies (one
  `i64.mul` ≈ 7.7k AND gates, and `mod_mul` is ~3 schoolbook 64×64 products),
  which wants upstream native-bignum/RAM work first. Params were found by a
  one-off search (`ecdsa-core/tests/gen.rs`, `#[ignore]`d): the j=0 family has
  CM by √−3, so orders come in closed form from `4p = L² + 27M²` (modified
  Cornacchia) and are verified rigorously (prime `n` in the Hasse interval +
  `n·P = O`); `parameters_are_consistent` re-checks the hardcoded constants
  every run. The advice pattern (as in transcript) carries the data-dependent
  and expensive parts as private hints the guest only CHECKS branch-free:
  field inverses (1/(2y), 1/(x₂−x₁), and `s⁻¹ mod n`) are supplied and pinned
  via `denom·inv = 1` (a prime field forces them exact; bad advice can only
  flip the verdict to 0); `u₁·G + u₂·Q` is a fixed-window comb (8 doublings +
  16 masked-merge table additions + 1 public correction) over a 256-entry/base
  public table both parties derive from Q off the VM, with fixed offsets so no
  honest affine point hits infinity. Both moduli are pseudo-Mersenne, so
  reduction is fold-and-add, no Montgomery. The host side (`feature = "host"`,
  never linked into the VM) signs, builds the table, and generates advice by
  replaying the SAME `walk` as the in-VM verifier — they can't drift; native
  tests differential-check `verify_slices` against a textbook verifier and
  assert every one of the 28 advice words is load-bearing. Both moduli's
  `mod_mul` and the comb live in `ecdsa-core` so they're shared and tested off
  the VM. See the memory-authentication rule above: the guest reads MSG/TABLE/
  ADVICE via accessors (no large stack arrays).
- Remote verifier (`web/src/remote.ts` + the remote section of main.ts;
  flag `remote`, default on, `?remote=0`): the page relay is the single
  point where messages cross parties, so a second device is page-level
  rewiring only — the host (prover) device shows a QR of `?join=<peer-id>`,
  the scanning device becomes the verifier, and each page pumps its own
  party's MessagePort into one reliable PeerJS DataChannel instead of into
  the other local worker. Workers/bindings unchanged; both workers still
  spawn on both devices (the idle one serves `guest_info`, so each device
  shows its own independently computed hash). Only signaling touches the
  PeerJS cloud broker; the protocol flows P2P (LAN-local on a shared
  network; no TURN, so client-isolated guest Wi-Fi blocks it — hotspot is
  the workaround). Wire format: 1 tag byte, then protocol bytes verbatim
  or control JSON with explicit typed-array encoding (run requests carry
  Uint8Array/Uint32Array/BigInt64Array); PeerJS binary serialization
  chunks >16 KB frames, so the 1 MB custom-wasm request needs no extra
  care. Control flow: the guest's pkg version rides in connection
  metadata and the host answers `hello` (version mismatch = different
  embedded guests → refuse); Run on the host sends `start` carrying the
  verifier request (public data only, same zeroed private fields as local
  mode) + blind string + claim labels (`RemoteDisplay`, consumed by the
  json/transcript render overrides) + summary log lines; `done`/`error`/
  `abort` mirror to the peer. Both directions go through `deliver()`, so
  traffic counters and the tamper button work per-device — but the tamper
  is hidden on the verifier (guest) device (`body.remote-guest .cheat`):
  the tamper only fires on the prover→verifier direction, and the guest
  would merely corrupt its own inbound bytes and self-reject. Only the
  prover's device tamper is a true MITM (corrupt before send); the
  verifier is the checker, so the button lives with the prover. While
  connected, the remote party's pane is replaced by a
  placeholder (`.remote-placeholder`, toggled via `remote-host`/
  `remote-guest` body classes) — the local UI in that pane would show
  this page's inputs, not the actual remote party's; the pane's log stays
  (it carries the remote progress lines). Gotchas: a `start` can beat
  worker readiness
  (`pendingStart`) and protocol bytes race ahead of it (`pendingBytes`,
  flushed in order); a broker blip after connect must NOT tear the link
  down (no peer-level error handler inside RemoteLink); `RemoteLink.close()`
  suppresses its own teardown event, so deliberate disconnects invoke
  `remoteEvents.onClose` themselves.
- Feature flags: `web/src/config.ts` — only `remote` remains (default on,
  `?remote=0` to disable). The tamper button now ships on by default (the
  last flag, `cheat`, was removed once the every-message corruption was
  verified to reject on every program — see the relay note above). The WAT
  editor was replaced by the "custom wasm" tab (default on): drop a compiled guest,
  the prover worker inspects it (`module_exports`), the page builds one
  input per argument with a private/public toggle, and the generic
  `prover_custom`/`verifier_custom` entry points call any exported
  function over i32/i64 scalars. The verifier request carries zeroed
  private values. NOTE: never name a `#[wasm_bindgen]` parameter `wasm` —
  the generated glue shadows its own `wasm` instance object.
  The latency slider is always shown; the step-through-messages mode
  was dropped. The Run button morphs into Abort during a run (abort
  terminates and respawns both workers — the protocol can't be interrupted
  any other way).

## Status / open items

- Repo: github.com/privacy-ethereum/speakup, the demo lives under `demo/`
  (imported from tlsnotary/speakup-demo with full history).
- GitHub Pages: the root `.github/workflows/pages.yml` builds the Sphinx
  docs AND the wasm pkg + vite site on every push to main, then deploys
  both as one artifact (docs at the root, demo under `/demo/`). Both build
  jobs always run — a single-artifact deploy would otherwise wipe the
  other half of the site. Repo settings → Pages → source must be "GitHub
  Actions". Vite base is `/speakup/demo/`.
- Perf anchors (Chrome, M-series 18 cores → 9 threads/party, threaded build
  + mpz c215974 + wasm-simd aes patch): square ≈ 0.08 s; regex email ≈ 0.28 s;
  csv 4 rows ≈ 0.11 s; transcript (jsonplaceholder_post, 1543 private bytes)
  ≈ 0.37 s / 140 msgs / ~1.5 MB; sha-256 16 KB ≈ 3.0 s / 206 msgs / ~2.5 MB
  (msg counts post write-coalescing — pre-coalescing 270 / 400);
  64 KB ≈ 11 s (OOM-aborted before c215974); 128 KB still OOMs;
  ecdsa (toy 64-bit curve, 39-byte msg) ≈ 4.5 s / 255 msgs / ~3.2 MB.
  Pre-aes-patch: square 0.10 s, regex 0.31 s, transcript 0.42 s, sha-256 16 KB
  3.7 s, 64 KB 13 s. Pre-threads baselines: square 0.4 s, sha-256 16 KB
  10.7 s, regex 1.6 s.
- Possible next: a wider-field ECDSA (P-256) once the VM gains native bignums
  or RAM (~100× the multiplies today); guided stepper narrative, more polish;
  Speakup RAM support will eventually allow private-offset designs (see mpz
  age-span discussion) and may relax the black_box discipline if `select`
  lands.
