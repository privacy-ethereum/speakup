//! DFA table layout and the branch-free (oblivious) matcher.
//!
//! See the `regex-guest` crate docs for the full picture. This crate holds
//! the parts that both sides of the protocol share:
//!
//! - the **table layout** the host serializes into and the guest reads;
//! - [`dfa_matches`], the matcher itself, written so that it never branches
//!   on, indexes by, or shifts by the (symbolic) input bytes — only on the
//!   (concrete, public) table words.
//!
//! Run on the zk-vm, `bytes` are symbolic; run natively, the same code is a
//! plain DFA evaluation, which is how the host cross-checks tables and how
//! the unit tests work.

/// Maximum DFA states (one-hot in a `u32`).
pub const MAX_STATES: usize = 32;
/// Maximum byte classes.
pub const MAX_CLASSES: usize = 16;
/// Maximum byte ranges per class.
pub const MAX_RANGES: usize = 8;
/// Maximum test-string length.
pub const INPUT_CAP: usize = 256;

/// Table layout (u32 words):
/// `[n_states, n_classes, start_idx, accept_mask]`, then per class
/// `MAX_RANGES` `(lo, hi)` pairs (unused ranges are `(1, 0)`: never match),
/// then the `MAX_STATES x MAX_CLASSES` next-state indices.
pub const HEADER_LEN: usize = 4;
pub const RANGES_BASE: usize = HEADER_LEN;
pub const RANGES_LEN: usize = MAX_CLASSES * MAX_RANGES * 2;
pub const TRANS_BASE: usize = RANGES_BASE + RANGES_LEN;
pub const TABLE_LEN: usize = TRANS_BASE + MAX_STATES * MAX_CLASSES;

/// Runs the DFA in `table` over `bytes`, branch-free in the input bytes.
/// Returns 1 if the whole string matches, else 0.
pub fn dfa_matches(table: &[u32; TABLE_LEN], bytes: &[u8]) -> i32 {
    let n_states = (table[0] as usize).min(MAX_STATES);
    let n_classes = (table[1] as usize).min(MAX_CLASSES);
    let accept = table[3];

    // One-hot current state; the start index is public, so this is concrete.
    let mut state: u32 = 1u32 << (table[2] % MAX_STATES as u32);

    for &byte in bytes {
        let b = byte as u32; // symbolic on the VM

        // Byte-class membership flags: range comparisons, never an index.
        let mut flags = [0u32; MAX_CLASSES];
        for (c, flag) in flags.iter_mut().enumerate().take(n_classes) {
            let mut m = 0u32;
            for r in 0..MAX_RANGES {
                let base = RANGES_BASE + (c * MAX_RANGES + r) * 2;
                let (lo, hi) = (table[base], table[base + 1]);
                m |= ((b >= lo) as u32) & ((b <= hi) as u32);
            }
            // Barrier: keep the flag as data, not a branch.
            *flag = core::hint::black_box(m);
        }

        // Transition: every edge contributes via masks; `1 << s` and
        // `1 << to` are concrete (public table, public loop indices).
        let mut next = 0u32;
        for s in 0..n_states {
            let s_bit = ((state & (1u32 << s)) != 0) as u32;
            for (c, flag) in flags.iter().enumerate().take(n_classes) {
                let to = table[TRANS_BASE + s * MAX_CLASSES + c] % MAX_STATES as u32;
                // -(0|1) is 0 or all-ones: a select without a select.
                next |= 0u32.wrapping_sub(s_bit & flag) & (1u32 << to);
            }
        }
        state = core::hint::black_box(next);
    }

    ((state & accept) != 0) as i32
}

/// Decodes a serialized table (LE `u32` bytes, as produced by the host's
/// builder) into the word array [`dfa_matches`] takes.
pub fn decode_table(bytes: &[u8]) -> [u32; TABLE_LEN] {
    let mut table = [0u32; TABLE_LEN];
    for (slot, chunk) in table.iter_mut().zip(bytes.chunks_exact(4)) {
        *slot = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built DFA for `a+b`: state 0 --a--> 1, 1 --a--> 1, 1 --b--> 2
    /// (accept); class 0 = 'a', class 1 = 'b', class 2 = everything else;
    /// missing transitions go to the dead state 3.
    fn a_plus_b() -> [u32; TABLE_LEN] {
        let mut t = [0u32; TABLE_LEN];
        t[0] = 4;
        t[1] = 3;
        t[2] = 0;
        t[3] = 1 << 2;
        let range = |c: usize, r: usize| RANGES_BASE + (c * MAX_RANGES + r) * 2;
        for i in (RANGES_BASE..TRANS_BASE).step_by(2) {
            t[i] = 1; // (1, 0): never matches
        }
        t[range(0, 0)] = b'a' as u32;
        t[range(0, 0) + 1] = b'a' as u32;
        t[range(1, 0)] = b'b' as u32;
        t[range(1, 0) + 1] = b'b' as u32;
        t[range(2, 0)] = 0;
        t[range(2, 0) + 1] = b'a' as u32 - 1;
        t[range(2, 1)] = b'b' as u32 + 1;
        t[range(2, 1) + 1] = 255;

        let tr = |s: usize, c: usize| TRANS_BASE + s * MAX_CLASSES + c;
        for s in 0..4 {
            for c in 0..3 {
                t[tr(s, c)] = 3;
            }
        }
        t[tr(0, 0)] = 1;
        t[tr(1, 0)] = 1;
        t[tr(1, 1)] = 2;
        t
    }

    #[test]
    fn matches_a_plus_b() {
        let t = a_plus_b();
        assert_eq!(dfa_matches(&t, b"aab"), 1);
        assert_eq!(dfa_matches(&t, b"ab"), 1);
        assert_eq!(dfa_matches(&t, b"b"), 0);
        assert_eq!(dfa_matches(&t, b"aaba"), 0);
        assert_eq!(dfa_matches(&t, b""), 0);
        assert_eq!(dfa_matches(&t, b"axb"), 0);
    }
}
