//! Compiles a (public) regex into the guest's DFA table format.
//!
//! This is the host half of the oblivious regex matcher: `regex-automata`
//! builds an anchored dense DFA with byte-class compression, we BFS its
//! reachable states, and serialize the result into the fixed layout the
//! `regex-guest` expects (see its crate docs). The table is *public* — both
//! parties build it independently from the same pattern and write identical
//! bytes into guest memory.
//!
//! Demo limits (the guest one-hot encodes states in a `u32`):
//! at most 32 DFA states, 16 byte classes, 8 ranges per class.

use std::collections::HashMap;

use regex_automata::{
    Anchored,
    dfa::{Automaton, StartKind, dense},
    util::syntax,
};
use regex_dfa_core::{MAX_CLASSES, MAX_RANGES, MAX_STATES, RANGES_BASE, TABLE_LEN, TRANS_BASE};

/// Builds the guest DFA table for `pattern`. Whole-string (anchored) match
/// semantics, byte-oriented (no unicode classes).
pub fn build_table(pattern: &str) -> Result<Vec<u8>, String> {
    let dfa = dense::Builder::new()
        .configure(
            dense::Config::new()
                .start_kind(StartKind::Anchored)
                .byte_classes(true)
                .minimize(true),
        )
        .syntax(syntax::Config::new().unicode(false).utf8(false))
        .build(pattern)
        .map_err(|e| format!("invalid pattern: {e}"))?;

    let start = dfa
        .universal_start_state(Anchored::Yes)
        .ok_or("unsupported pattern (look-around?)")?;

    let classes = dfa.byte_classes();
    // The last "class" is the end-of-input sentinel; the guest never sees it.
    let n_classes = classes.alphabet_len() - 1;
    if n_classes > MAX_CLASSES {
        return Err(format!(
            "pattern too complex: {n_classes} byte classes (demo max {MAX_CLASSES})"
        ));
    }

    // A representative byte per class (a class can be empty; then no byte
    // ever selects it and its transitions are irrelevant).
    let mut reps: Vec<Option<u8>> = vec![None; n_classes];
    for b in 0..=255u8 {
        let c = classes.get(b) as usize;
        if c < n_classes && reps[c].is_none() {
            reps[c] = Some(b);
        }
    }

    // BFS the reachable states; our state index = discovery order.
    let mut ids = vec![start];
    let mut index = HashMap::from([(start, 0usize)]);
    let mut i = 0;
    while i < ids.len() {
        let sid = ids[i];
        for rep in reps.iter().flatten() {
            let next = dfa.next_state(sid, *rep);
            if !index.contains_key(&next) {
                if ids.len() == MAX_STATES {
                    return Err(format!(
                        "pattern too complex: more than {MAX_STATES} DFA states"
                    ));
                }
                index.insert(next, ids.len());
                ids.push(next);
            }
        }
        i += 1;
    }

    let mut t = vec![0u32; TABLE_LEN];
    t[0] = ids.len() as u32;
    t[1] = n_classes as u32;
    t[2] = 0; // the start state is discovered first
    for (i, sid) in ids.iter().enumerate() {
        // Accepting iff feeding end-of-input from here lands on a match.
        if dfa.is_match_state(dfa.next_eoi_state(*sid)) {
            t[3] |= 1 << i;
        }
    }

    // Class ranges; unused slots stay (1, 0): they never match.
    for c in 0..MAX_CLASSES {
        for r in 0..MAX_RANGES {
            t[RANGES_BASE + (c * MAX_RANGES + r) * 2] = 1;
        }
    }
    for c in 0..n_classes {
        let mut ranges: Vec<(u8, u8)> = Vec::new();
        for b in 0..=255u8 {
            if classes.get(b) as usize == c {
                match ranges.last_mut() {
                    Some((_, hi)) if *hi as u16 + 1 == b as u16 => *hi = b,
                    _ => ranges.push((b, b)),
                }
            }
        }
        if ranges.len() > MAX_RANGES {
            return Err(format!(
                "pattern too complex: a byte class spans {} ranges (demo max {MAX_RANGES})",
                ranges.len()
            ));
        }
        for (r, (lo, hi)) in ranges.iter().enumerate() {
            let base = RANGES_BASE + (c * MAX_RANGES + r) * 2;
            t[base] = *lo as u32;
            t[base + 1] = *hi as u32;
        }
    }

    for (i, sid) in ids.iter().enumerate() {
        for (c, rep) in reps.iter().enumerate() {
            let Some(rep) = rep else { continue };
            let next = dfa.next_state(*sid, *rep);
            t[TRANS_BASE + i * MAX_CLASSES + c] = index[&next] as u32;
        }
    }

    Ok(t.iter().flat_map(|w| w.to_le_bytes()).collect())
}

// Correctness of the builder + guest matcher pair is covered in
// tests/browser.rs (this crate only targets wasm).
