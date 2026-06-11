# Speakup demo — working notes

Browser demo of Speakup (the mpz zk-vm): prover and verifier in separate web
workers, real OT stack, six example guests. See README.md for the user-facing
picture; this file is for working on the code.

## Build & test

```sh
# guests: native unit tests (mpz-vm-sys is a no-op off the VM)
cd guests && cargo test

# bindings: e2e browser tests (headless Chrome) — the real test suite
cd rust && wasm-pack test --headless --chrome --release

# rebuild the web pkg after ANY rust/ or guests/ change
cd rust && wasm-pack build --release --target web --out-dir ../web/src/pkg

# web app
cd web && npm run dev          # add --host to test from other devices
cd web && ./node_modules/.bin/tsc --noEmit   # type check (vite doesn't)
```

`rust/build.rs` compiles `guests/` to wasm32 automatically (artifacts land in
OUT_DIR; `lib.rs` embeds them) — editing a guest and rebuilding the pkg is one
step. mpz comes from git, branch `v2`, fetched with the git CLI (libgit2
chokes on the repo); `Cargo.lock` pins the exact rev.

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
- Guest crates must NEVER be linked into `rust/` (mpz-vm-sys emits `vc.*`
  wasm imports nothing satisfies). Shared logic goes in a separate crate with
  no mpz-vm-sys dep — see `guests/regex-core`.
- Feature flags: `web/src/config.ts`, URL overrides `?cheat=1&wat=1` —
  tamper button and WAT editor (default off, undecided whether they ship).
  The relay-delay slider is always shown; the step-through-messages mode
  was dropped. The Run button morphs into Abort during a run (abort
  terminates and respawns both workers — the protocol can't be interrupted
  any other way).

## Status / open items

- Repo: github.com/tlsnotary/speakup-demo (private).
- GitHub Pages deploy deferred: org has no paid plan, so Pages can't serve a
  private repo. Vite base `/speakup-demo/` is configured and the production
  build is verified; revisit when the repo goes public (or use
  Cloudflare/Netlify for private repos).
- Perf anchors (Chrome, M-series): square ≈ 0.4 s / 21 msgs / ~900 KB;
  sha-256 16 KB ≈ 10.7 s / 3.1 MB; regex email ≈ 1.6 s; csv 4 rows ≈ 1.6 s.
- Possible next: guided stepper narrative, deploy, more polish; Speakup RAM
  support will eventually allow private-offset designs (see mpz age-span
  discussion) and may relax the black_box discipline if `select` lands.
