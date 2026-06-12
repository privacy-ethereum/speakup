//! A Speakup demo guest: selective disclosure over an HTTP transcript.
//!
//! The host parses the transcript ONCE outside the VM (with the real
//! `transcript-verify` host parser) and writes the resulting span table —
//! plus the public claims: method, target, `Host`, status, and a JSON
//! path — into [`TABLE`] as **public** words. Public data stays concrete
//! inside the VM, so all control flow (header loops, the JSON tree walk)
//! is driven by it. The raw `sent`/`recv` bytes are **private**; the
//! verifier learns only the table (the *structure* of the exchange), the
//! claims, and the disclosed value.
//!
//! The verification logic lives in [`transcript_core`] (shared with the
//! host, which cross-checks every encoding natively): the public table
//! drives a cursor over the private bytes and every claim is re-derived
//! branch-free — literal anchors, charset gates, header tiling,
//! Content-Length digits, the full JSON grammar — into one `ok` flag.
//!
//! Revealed: the 0/1 flag and — in disclose mode — the bytes of the JSON
//! value at the public path (masked to zeros unless `ok`). In assert mode
//! the flag alone: "the value equals the public expected string" is folded
//! into it. Everything else stays hidden.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use transcript_core::{OUT_CAP, RECV_CAP, SENT_CAP, TABLE_CAP_WORDS};

/// The flat span table + claims, written by the host as public words.
static TABLE: [AtomicU32; TABLE_CAP_WORDS] = [const { AtomicU32::new(0) }; TABLE_CAP_WORDS];

/// The request bytes, written by the prover as private bytes.
static SENT: [AtomicU8; SENT_CAP] = [const { AtomicU8::new(0) }; SENT_CAP];

/// The response bytes, written by the prover as private bytes.
static RECV: [AtomicU8; RECV_CAP] = [const { AtomicU8::new(0) }; RECV_CAP];

/// The disclosed value bytes (masked to zeros on failure), revealed in
/// place; the host reads them back after the call.
static OUT: [AtomicU8; OUT_CAP] = [const { AtomicU8::new(0) }; OUT_CAP];

/// Address of the public table buffer.
#[no_mangle]
pub extern "C" fn table_ptr() -> i32 {
    TABLE.as_ptr() as i32
}

/// Address of the private request buffer.
#[no_mangle]
pub extern "C" fn sent_ptr() -> i32 {
    SENT.as_ptr() as i32
}

/// Address of the private response buffer.
#[no_mangle]
pub extern "C" fn recv_ptr() -> i32 {
    RECV.as_ptr() as i32
}

/// Address of the disclosure output buffer.
#[no_mangle]
pub extern "C" fn out_ptr() -> i32 {
    OUT.as_ptr() as i32
}

/// Loads the buffers, verifies the private transcript against the public
/// table, and reveals the 0/1 flag plus the masked value bytes.
#[no_mangle]
pub extern "C" fn verify_transcript(sent_len: i32, recv_len: i32, n_words: i32) -> i32 {
    let bb = core::hint::black_box::<i32>;

    let n_words = (n_words as u32 as usize).min(TABLE_CAP_WORDS);
    let mut table = [0u32; TABLE_CAP_WORDS];
    for (dst, slot) in table[..n_words].iter_mut().zip(TABLE.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    let sent_len = (sent_len as u32 as usize).min(SENT_CAP);
    let mut sent = [0u8; SENT_CAP];
    for (dst, slot) in sent[..sent_len].iter_mut().zip(SENT.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    let recv_len = (recv_len as u32 as usize).min(RECV_CAP);
    let mut recv = [0u8; RECV_CAP];
    for (dst, slot) in recv[..recv_len].iter_mut().zip(RECV.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }

    // The early return keeps the Ok/Err merge out of LLVM's hands: a
    // `match` here lowers to `select`s whose arms include the SYMBOLIC ok
    // flag, which the VM rejects even under a concrete condition. A
    // malformed table is public, so both parties take the same concrete
    // path (no reveals, result 0).
    let d = match transcript_core::verify(&sent[..sent_len], &recv[..recv_len], &table[..n_words])
    {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let (ok, value_start, value_len) = (d.ok, d.value_start, d.value_len);

    // Disclose the value bytes, masked to zeros unless every check passed
    // (the span is public; the bytes are private until this reveal). In
    // assert mode `value_len` is 0 — only the flag is revealed; whether to
    // reveal is decided on PUBLIC data, so both parties skip identically.
    if value_len > 0 {
        // Which buffer holds the value is part of the public claim (a
        // request-header claim discloses from `sent`): a concrete branch,
        // taken identically by both parties.
        let src: &[u8] = if d.value_in_sent { &sent } else { &recv };
        let mask = 0i32.wrapping_sub(bb(ok));
        for i in 0..value_len {
            let b = src[value_start + i] as i32;
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
        assert_eq!(verify_transcript(4, 4, 4), 0);
    }
}
