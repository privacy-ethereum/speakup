//! A zk-vm demo guest: oblivious regex matching.
//!
//! Proves that a **private** test string matches a **public** regular
//! expression — without revealing the string. The host compiles the regex
//! (with `regex-automata`, outside the VM) into a dense DFA and writes its
//! transition table into [`TABLE`] as **public** bytes; public bytes stay
//! concrete inside the VM, so the guest can loop over and branch on the
//! table freely. Only the test string in [`INPUT`] is private.
//!
//! The matcher itself lives in the [`regex_dfa_core`] crate (shared with the
//! host, which uses it to cross-check tables off the VM): an oblivious DFA
//! evaluation over a one-hot state vector, branch-free in the input bytes.
//! This crate only adds the VM plumbing: the public/private buffers, their
//! address exports, and the VCI reveal of the final 0/1 flag.
//!
//! Revealed: a single 0/1 — whether the whole string matches. The verifier
//! learns the pattern (it's public) and the string's length, nothing else.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use regex_dfa_core::{INPUT_CAP, TABLE_LEN, dfa_matches};

/// The DFA table, written by the host as public bytes (concrete in the VM).
static TABLE: [AtomicU32; TABLE_LEN] = [const { AtomicU32::new(0) }; TABLE_LEN];

/// The test string, written by the prover as private bytes.
static INPUT: [AtomicU8; INPUT_CAP] = [const { AtomicU8::new(0) }; INPUT_CAP];

/// Address of the public DFA table buffer.
#[no_mangle]
pub extern "C" fn table_ptr() -> i32 {
    TABLE.as_ptr() as i32
}

/// Address of the private test-string buffer.
#[no_mangle]
pub extern "C" fn input_ptr() -> i32 {
    INPUT.as_ptr() as i32
}

/// Loads the table and `len` private bytes from the buffers, runs the
/// oblivious DFA, and reveals only the 0/1 match flag.
#[no_mangle]
pub extern "C" fn matches(len: i32) -> i32 {
    let mut table = [0u32; TABLE_LEN];
    for (dst, slot) in table.iter_mut().zip(TABLE.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }

    let len = (len as u32 as usize).min(INPUT_CAP);
    let mut bytes = [0u8; INPUT_CAP];
    for (dst, slot) in bytes[..len].iter_mut().zip(INPUT.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }

    mpz_vm_sys::reveal(dfa_matches(&table, &bytes[..len])).wait()
}
