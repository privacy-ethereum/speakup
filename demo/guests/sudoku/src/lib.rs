//! A Sudoku-solution guest for the zk-vm.
//!
//! The host stages two buffers: an 81-cell **public** puzzle (`0` = blank,
//! `1..=9` = a given clue) and an 81-cell **private** solution, followed by one
//! spare byte for the verdict. [`check`] verifies — entirely inside the VM —
//! that the private solution completes the public puzzle and is a valid grid,
//! then reveals a single `1`/`0` byte. The solution itself is never revealed,
//! so the prover demonstrates knowledge of a solution without disclosing it.
//!
//! ## Staying branch-free on private data
//!
//! The VM cannot branch on, index by, shift by, or `select` on the (symbolic)
//! solution cells, so [`validate`] uses none of those: it only loads cells at
//! **public** indices and accumulates an integer count of constraint
//! *violations* from comparisons. A cell is in range iff `!(v < 1 || v > 9)`;
//! a group (row / column / box) is a permutation of `1..=9` iff its nine cells
//! are pairwise distinct and all in range — checked with `==` over public index
//! pairs, never a per-value bitmask (which would need a private shift). Each
//! 0/1 flag is pinned with `black_box` so LLVM can't re-introduce a `select`
//! or a private branch (`x += cond` → `select(cond, x+1, x)`), and the final
//! `violations == 0` is a scalar `i32.eqz`, not a byte-array `bcmp`.

use std::hint::black_box;

const N: usize = 81;

/// The nine cell indices of row `r` (`r` is public).
fn row_idx(r: usize) -> [usize; 9] {
    let mut idx = [0usize; 9];
    let mut c = 0;
    while c < 9 {
        idx[c] = r * 9 + c;
        c += 1;
    }
    idx
}

/// The nine cell indices of column `c` (`c` is public).
fn col_idx(c: usize) -> [usize; 9] {
    let mut idx = [0usize; 9];
    let mut r = 0;
    while r < 9 {
        idx[r] = r * 9 + c;
        r += 1;
    }
    idx
}

/// The nine cell indices of 3×3 box `b` (`b` is public), in row-major order.
fn box_idx(b: usize) -> [usize; 9] {
    let br = (b / 3) * 3;
    let bc = (b % 3) * 3;
    let mut idx = [0usize; 9];
    let mut k = 0;
    let mut dr = 0;
    while dr < 3 {
        let mut dc = 0;
        while dc < 3 {
            idx[k] = (br + dr) * 9 + (bc + dc);
            k += 1;
            dc += 1;
        }
        dr += 1;
    }
    idx
}

/// Count duplicate pairs among the nine cells at `idx` (each pair contributes
/// one violation). Cells are read at public indices; only the values are
/// symbolic, so every comparison is pinned to stop LLVM lowering the running
/// sum to a `select`.
fn group_dups(solution: &[u8], idx: &[usize; 9]) -> i32 {
    let mut cells = [0i32; 9];
    let mut k = 0;
    while k < 9 {
        cells[k] = solution[idx[k]] as i32;
        k += 1;
    }
    let mut dups = 0i32;
    let mut i = 0;
    while i < 9 {
        let mut j = i + 1;
        while j < 9 {
            dups += black_box((cells[i] == cells[j]) as i32);
            j += 1;
        }
        i += 1;
    }
    dups
}

/// Returns whether `solution` is a valid completion of `puzzle`. Both slices
/// must be 81 cells. Branch-free over the (private) solution values.
pub fn validate(puzzle: &[u8], solution: &[u8]) -> bool {
    let mut violations: i32 = 0;

    // 1. Every cell is in 1..=9.
    let mut i = 0;
    while i < N {
        let v = solution[i] as i32;
        violations += black_box((v < 1) as i32) + black_box((v > 9) as i32);
        i += 1;
    }

    // 2. The solution agrees with every public clue (puzzle cell != 0). The
    //    branch is on public data, so it is fine.
    let mut i = 0;
    while i < N {
        let clue = puzzle[i] as i32;
        if clue != 0 {
            let v = solution[i] as i32;
            violations += black_box((v != clue) as i32);
        }
        i += 1;
    }

    // 3. Every row, column, and 3×3 box has nine distinct values. With the
    //    range check above, distinct ⇒ a permutation of 1..=9.
    let mut g = 0;
    while g < 9 {
        violations += group_dups(solution, &row_idx(g));
        violations += group_dups(solution, &col_idx(g));
        violations += group_dups(solution, &box_idx(g));
        g += 1;
    }

    black_box(violations) == 0
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{validate, N};
    use std::alloc::Layout;

    /// The Component Model canonical `realloc`:
    /// `cabi_realloc(old_ptr, old_size, align, new_size) -> ptr`.
    #[no_mangle]
    pub extern "C" fn cabi_realloc(old_ptr: i32, old_size: i32, align: i32, new_size: i32) -> i32 {
        let align = (align as usize).max(1);
        let new_size = new_size as usize;
        unsafe {
            if old_ptr == 0 {
                let layout = Layout::from_size_align_unchecked(new_size, align);
                std::alloc::alloc(layout) as i32
            } else {
                let old_layout = Layout::from_size_align_unchecked(old_size as usize, align);
                std::alloc::realloc(old_ptr as *mut u8, old_layout, new_size) as i32
            }
        }
    }

    /// Returns `1` if the private solution at `sol_ptr` (81 cells) is a valid
    /// completion of the public puzzle at `puzzle_ptr` (81 cells), else `0`. The
    /// verdict is the call's return value — revealed to both parties by the
    /// interactive call — so the solution itself is never disclosed.
    #[no_mangle]
    pub extern "C" fn check(puzzle_ptr: i32, sol_ptr: i32) -> i32 {
        let puzzle = unsafe { core::slice::from_raw_parts(puzzle_ptr as *const u8, N) };
        let solution = unsafe { core::slice::from_raw_parts(sol_ptr as *const u8, N) };
        validate(puzzle, solution) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    const PUZZLE: &[u8; 81] = &[
        5, 3, 0, 0, 7, 0, 0, 0, 0, //
        6, 0, 0, 1, 9, 5, 0, 0, 0, //
        0, 9, 8, 0, 0, 0, 0, 6, 0, //
        8, 0, 0, 0, 6, 0, 0, 0, 3, //
        4, 0, 0, 8, 0, 3, 0, 0, 1, //
        7, 0, 0, 0, 2, 0, 0, 0, 6, //
        0, 6, 0, 0, 0, 0, 2, 8, 0, //
        0, 0, 0, 4, 1, 9, 0, 0, 5, //
        0, 0, 0, 0, 8, 0, 0, 7, 9, //
    ];

    const SOLUTION: &[u8; 81] = &[
        5, 3, 4, 6, 7, 8, 9, 1, 2, //
        6, 7, 2, 1, 9, 5, 3, 4, 8, //
        1, 9, 8, 3, 4, 2, 5, 6, 7, //
        8, 5, 9, 7, 6, 1, 4, 2, 3, //
        4, 2, 6, 8, 5, 3, 7, 9, 1, //
        7, 1, 3, 9, 2, 4, 8, 5, 6, //
        9, 6, 1, 5, 3, 7, 2, 8, 4, //
        2, 8, 7, 4, 1, 9, 6, 3, 5, //
        3, 4, 5, 2, 8, 6, 1, 7, 9, //
    ];

    #[test]
    fn accepts_the_solution() {
        assert!(validate(PUZZLE, SOLUTION));
    }

    #[test]
    fn rejects_a_solution_disagreeing_with_a_clue() {
        // Clue at cell 0 is 5; change the solution there to a 1 (and fix the
        // duplicate it would create so only the clue mismatch remains).
        let mut bad = *SOLUTION;
        bad[0] = 1; // was 5, but cell 7 in the row is already 1 → also a dup
        assert!(!validate(PUZZLE, &bad));
    }

    #[test]
    fn rejects_a_duplicate_in_a_row() {
        let mut bad = *SOLUTION;
        bad[8] = bad[7]; // row 0 now has two of the same value
        assert!(!validate(PUZZLE, &bad));
    }

    #[test]
    fn rejects_an_out_of_range_cell() {
        let empty_puzzle = [0u8; 81];
        let mut bad = *SOLUTION;
        bad[40] = 0; // 0 is out of 1..=9
        assert!(!validate(&empty_puzzle, &bad));
    }

    #[test]
    fn accepts_against_an_empty_puzzle() {
        // No clues to satisfy — a valid grid alone should pass.
        assert!(validate(&[0u8; 81], SOLUTION));
    }
}
