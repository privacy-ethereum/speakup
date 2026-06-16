//! A Speakup demo guest: verify an ECDSA signature inside the zk-vm.
//!
//! The prover stages a **private** message and a **private** signature
//! (r, s) — plus the advice the verifier-side checks force to be honest:
//! s⁻¹ mod n and one field inverse per point operation (see ecdsa-core's
//! crate docs for why inverses are advice on a VM that cannot branch on
//! private data). Both parties stage the same **public** comb table,
//! derived off the VM from the public key. Only the 0/1 verdict is
//! revealed: the verifier learns the message length and the key — never
//! the message or the signature.
//!
//! The curve is a deliberately toy 64-bit sibling of secp256k1
//! (`y² = x³ + 2` over `2^64 − 453`); the verification algorithm is the
//! real thing.

use core::sync::atomic::{AtomicU8, Ordering};
use ecdsa_core::{ADVICE_BYTES, MSG_CAP, TABLE_BYTES};

/// The private message bytes.
static MSG: [AtomicU8; MSG_CAP] = [const { AtomicU8::new(0) }; MSG_CAP];
/// The private signature and advice words (r, s, s⁻¹, step inverses), LE.
static ADVICE: [AtomicU8; ADVICE_BYTES] = [const { AtomicU8::new(0) }; ADVICE_BYTES];
/// The public comb table (both parties write identical copies), LE words.
static TABLE: [AtomicU8; TABLE_BYTES] = [const { AtomicU8::new(0) }; TABLE_BYTES];

#[no_mangle]
pub extern "C" fn msg_ptr() -> i32 {
    MSG.as_ptr() as i32
}

#[no_mangle]
pub extern "C" fn advice_ptr() -> i32 {
    ADVICE.as_ptr() as i32
}

#[no_mangle]
pub extern "C" fn table_ptr() -> i32 {
    TABLE.as_ptr() as i32
}

/// Reads one little-endian u64 word from a static byte buffer.
fn word(bytes: &[AtomicU8], i: usize) -> u64 {
    let mut le = [0u8; 8];
    for (j, b) in le.iter_mut().enumerate() {
        *b = bytes[i * 8 + j].load(Ordering::Relaxed);
    }
    u64::from_le_bytes(le)
}

/// Verifies the staged signature over the staged message (`len` public
/// bytes) and reveals only the 0/1 verdict.
///
/// `ecdsa_core::verify` reads through accessors so the guest scans the
/// authenticated VM buffers (`MSG`/`TABLE`/`ADVICE`) in place — never
/// copying them into a stack array. A large array move would lower to
/// `memory.copy`, whose bytes the zk-vm doesn't authenticate, faulting with
/// `MemAuthMissing` on read-back. Only `word`'s tiny 8-byte `le` and
/// SHA-256's one `block` are stack arrays, and both are fully overwritten
/// before any read (the one zk-vm-safe array shape).
#[no_mangle]
pub extern "C" fn verify_sig(len: i32) -> i32 {
    let len = (len as u32 as usize).min(MSG_CAP);
    let ok = ecdsa_core::verify(
        len,
        |i| MSG[i].load(Ordering::Relaxed),
        |i| word(&TABLE, i),
        |i| word(&ADVICE, i),
    );
    mpz_vm_sys::reveal(ok as i32).wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecdsa_core::host;

    fn stage(msg: &[u8], table: &[u64], advice: &[u64]) {
        for (slot, b) in MSG.iter().zip(msg) {
            slot.store(*b, Ordering::Relaxed);
        }
        for (i, w) in table.iter().enumerate() {
            for (j, b) in w.to_le_bytes().iter().enumerate() {
                TABLE[i * 8 + j].store(*b, Ordering::Relaxed);
            }
        }
        for (i, w) in advice.iter().enumerate() {
            for (j, b) in w.to_le_bytes().iter().enumerate() {
                ADVICE[i * 8 + j].store(*b, Ordering::Relaxed);
            }
        }
    }

    // Built natively, `reveal(ok).wait()` hands `ok` straight back, so the
    // whole guest path can be exercised off the VM.
    #[test]
    fn accepts_a_valid_signature_and_rejects_a_tampered_one() {
        let q = (ecdsa_core::QX, ecdsa_core::QY);
        let table = host::tables(q);
        let msg = b"attest: the prover holds a validly signed message";
        let (r, s) = host::sign(ecdsa_core::D_DEMO, msg);

        stage(msg, &table, &host::advice(&table, msg, r, s));
        assert_eq!(verify_sig(msg.len() as i32), 1);

        stage(msg, &table, &host::advice(&table, msg, r, s ^ 2));
        assert_eq!(verify_sig(msg.len() as i32), 0);
    }
}
