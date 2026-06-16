# age-guest — zero-knowledge age verification

A demo guest for the zk-vm. It is the zk-vm analog of TLSNotary's
[`examples-zk`](https://github.com/tlsnotary/tlsn/tree/main/crates/examples-zk):
the prover holds a birth date **privately** and proves they are **18 or older**
without disclosing the date itself.

For the simplest possible guest (a private `(x+1)²` over a single scalar), see
the sibling [`square`](../square) crate; this one shows passing a private
**byte string** through guest memory.

## How it works

```
  host                              guest (wasm, on the zk-vm)
  ────                              ──────────────────────────
  birthdate_ptr()  ───────────────▶ returns address of BIRTHDATE buffer
  write(ptr, Private "1985-03-12")  prover supplies the real bytes
  write(ptr, Blind  len=10)         verifier learns only the length
  is_adult(today=20240610) ───────▶ parse date, compare, reveal 0/1
                       flag ◀─────── only the boolean is disclosed
```

1. The host calls `birthdate_ptr()` to learn where the `BIRTHDATE` buffer lives
   (a public constant — only the *bytes* written there are private).
2. The **prover** writes the `"YYYY-MM-DD"` date as `Write::Private`; the
   **verifier** writes a `Write::Blind` region of the same length, so it learns
   the length but never the date.
3. The host calls `is_adult(today)` with today's date packed as `YYYYMMDD`. The
   guest parses the (symbolic) birth date and reveals a single `0/1` flag —
   `1` if the holder is 18+, else `0`. The birth date is never revealed.

The parse is deliberately **branch-free**: it only does arithmetic on the
private bytes and a final comparison, never a control-flow decision based on
them (which the VM cannot resolve locally). This is why the format is assumed to
be a well-formed 10-character `"YYYY-MM-DD"` — there is no input validation.

## Layout

| File | Purpose |
| --- | --- |
| `src/lib.rs` | The guest: `birthdate_ptr`, `is_adult`, and native unit tests. |
| `Cargo.toml` | Standalone crate, **excluded from the workspace** (built to wasm separately). |

The end-to-end test that drives this guest through the real `Prover`/`Verifier`
pair lives at [`crates/vm-zk/tests/age.rs`](../../vm-zk/tests/age.rs).

## Building

Native unit tests (the `mpz-vm-sys` bindings are clear-execution no-ops off the
VM, so `is_adult` reduces to the plain age check):

```sh
cargo test
```

Rebuild the wasm artifact used by the integration test:

```sh
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/age_guest.wasm \
   ../../vm-zk/tests/guests/age.wasm
```
