//! A Speakup demo guest: selective disclosure over a private JSON document.
//!
//! The JSON-only sibling of the transcript guest — the same advice pattern
//! with the HTTP layers dropped. The host parses the document ONCE outside
//! the VM (with the real `transcript-verify` JSON parser) and writes the
//! resulting node table — plus the public claim: a JSON path and the
//! assert/disclose mode — into [`TABLE`] as **public** words. Public data
//! stays concrete inside the VM, so the whole tree walk is driven by it.
//! The raw document bytes are **private**; the verifier learns only the
//! table (the *structure* of the document: nesting, spans, key lengths),
//! the claim, and the disclosed value.
//!
//! The verification logic lives in [`transcript_core::json`] (shared with
//! the host, which cross-checks every encoding natively): the public table
//! drives a cursor over the private bytes and the full JSON grammar is
//! re-derived branch-free into one `ok` flag.
//!
//! Revealed: the 0/1 flag and — in disclose mode — the bytes of the JSON
//! value at the public path (masked to zeros unless `ok`). In assert mode
//! the flag alone: "the value equals the public expected string" is folded
//! into it. Everything else stays hidden.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use transcript_core::{OUT_CAP, TABLE_CAP_WORDS, json::DOC_CAP};

/// The flat node table + claim, written by the host as public words.
static TABLE: [AtomicU32; TABLE_CAP_WORDS] = [const { AtomicU32::new(0) }; TABLE_CAP_WORDS];

/// The document bytes, written by the prover as private bytes.
static DOC: [AtomicU8; DOC_CAP] = [const { AtomicU8::new(0) }; DOC_CAP];

/// The disclosed value bytes (masked to zeros on failure), revealed in
/// place; the host reads them back after the call.
static OUT: [AtomicU8; OUT_CAP] = [const { AtomicU8::new(0) }; OUT_CAP];

/// Address of the public table buffer.
#[no_mangle]
pub extern "C" fn table_ptr() -> i32 {
    TABLE.as_ptr() as i32
}

/// Address of the private document buffer.
#[no_mangle]
pub extern "C" fn doc_ptr() -> i32 {
    DOC.as_ptr() as i32
}

/// Address of the disclosure output buffer.
#[no_mangle]
pub extern "C" fn out_ptr() -> i32 {
    OUT.as_ptr() as i32
}

/// Loads the buffers, verifies the private document against the public
/// table, and reveals the 0/1 flag plus the masked value bytes.
#[no_mangle]
pub extern "C" fn verify_json(doc_len: i32, n_words: i32) -> i32 {
    let bb = core::hint::black_box::<i32>;

    let n_words = (n_words as u32 as usize).min(TABLE_CAP_WORDS);
    let mut table = [0u32; TABLE_CAP_WORDS];
    for (dst, slot) in table[..n_words].iter_mut().zip(TABLE.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    let doc_len = (doc_len as u32 as usize).min(DOC_CAP);
    let mut doc = [0u8; DOC_CAP];
    for (dst, slot) in doc[..doc_len].iter_mut().zip(DOC.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }

    // The early return keeps the Ok/Err merge out of LLVM's hands: a
    // `match` here lowers to `select`s whose arms include the SYMBOLIC ok
    // flag, which the VM rejects even under a concrete condition. A
    // malformed table is public, so both parties take the same concrete
    // path (no reveals, result 0).
    let d = match transcript_core::json::verify(&doc[..doc_len], &table[..n_words]) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let (ok, value_start, value_len) = (d.ok, d.value_start, d.value_len);

    // Disclose the value bytes, masked to zeros unless every check passed
    // (the span is public; the bytes are private until this reveal). In
    // assert mode `value_len` is 0 — only the flag is revealed; whether to
    // reveal is decided on PUBLIC data, so both parties skip identically.
    if value_len > 0 {
        let mask = 0i32.wrapping_sub(bb(ok));
        for i in 0..value_len {
            let b = doc[value_start + i] as i32;
            OUT[i].store(bb(b & mask) as u8, Ordering::Relaxed);
        }
        let out = unsafe { core::slice::from_raw_parts(OUT.as_ptr() as *const u8, value_len) };
        let pending_out = mpz_vm_sys::reveal(out);
        let pending_ok = mpz_vm_sys::reveal(ok);
        pending_out.wait();
        pending_ok.wait()
    } else {
        mpz_vm_sys::reveal(ok).wait()
    }
}

#[cfg(test)]
mod tests {
    // The verification logic itself is tested in transcript-core; this
    // crate only adds the VM plumbing, which has no native behavior beyond
    // delegation. Smoke-test the delegation with an obviously bad table.
    use super::*;

    #[test]
    fn rejects_garbage_concretely() {
        assert_eq!(verify_json(4, 4), 0);
    }
}
