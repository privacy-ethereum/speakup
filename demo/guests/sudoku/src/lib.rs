//! A Speakup demo guest: zero-knowledge sudoku.
//!
//! The classic "I know a solution" proof. The puzzle (clues) is **public** —
//! both parties write it into [`PUZZLE`]; the prover's completed grid in
//! [`SOLUTION`] is **private**. The guest checks, branch-free in the private
//! cells, that:
//!
//!   - every row, column, and 3×3 box contains each value 1–9 exactly once
//!     (counted with equality flags and additions — never a lookup or shift
//!     indexed by a private cell);
//!   - the solution agrees with every public clue (branching on the *clue*
//!     is fine: it's public, hence concrete in the VM).
//!
//! Revealed: a single 0/1 — "this is a valid solution to the public puzzle".
//! The verifier learns nothing about the solution cells.
//!
//! Cells are stored as digit *values* (0–9, with 0 = empty in the puzzle),
//! not ASCII; the host converts.

use core::sync::atomic::{AtomicU8, Ordering};

/// Cells in the grid.
pub const GRID: usize = 81;

/// The public puzzle: 81 cells, 0 = empty, 1–9 = clue.
static PUZZLE: [AtomicU8; GRID] = [const { AtomicU8::new(0) }; GRID];

/// The prover's private solution: 81 cells, each 1–9.
static SOLUTION: [AtomicU8; GRID] = [const { AtomicU8::new(0) }; GRID];

/// Address of the public puzzle buffer.
#[no_mangle]
pub extern "C" fn puzzle_ptr() -> i32 {
    PUZZLE.as_ptr() as i32
}

/// Address of the private solution buffer.
#[no_mangle]
pub extern "C" fn solution_ptr() -> i32 {
    SOLUTION.as_ptr() as i32
}

/// Whether `sol` is a valid solution of `puzzle`; branch-free in the
/// (private) solution cells.
fn is_valid(puzzle: &[u8; GRID], sol: &[u8; GRID]) -> i32 {
    let s = |r: usize, c: usize| sol[r * 9 + c] as i32;

    // Nonzero as soon as any check fails. Every unit containing each of 1–9
    // exactly once also implies every cell is in 1..=9.
    let mut bad = 0u32;
    for v in 1..=9i32 {
        for u in 0..9 {
            let mut row = 0i32;
            let mut col = 0i32;
            let mut boxx = 0i32;
            for k in 0..9 {
                row += (s(u, k) == v) as i32;
                col += (s(k, u) == v) as i32;
                boxx += (s((u / 3) * 3 + k / 3, (u % 3) * 3 + k % 3) == v) as i32;
            }
            bad |= ((row != 1) as u32) | ((col != 1) as u32) | ((boxx != 1) as u32);
        }
        // Barrier per value: keep the accumulator as data, not a branch.
        bad = core::hint::black_box(bad);
    }

    // The solution must extend the public clues.
    for i in 0..GRID {
        let clue = puzzle[i]; // public, concrete: branching on it is fine
        if clue != 0 {
            bad |= (sol[i] != clue) as u32;
        }
    }

    (core::hint::black_box(bad) == 0) as i32
}

/// Loads both grids, checks the (private) solution against the (public)
/// puzzle, and reveals only the 0/1 validity flag.
#[no_mangle]
pub extern "C" fn check() -> i32 {
    let mut puzzle = [0u8; GRID];
    let mut sol = [0u8; GRID];
    for i in 0..GRID {
        puzzle[i] = PUZZLE[i].load(Ordering::Relaxed);
        sol[i] = SOLUTION[i].load(Ordering::Relaxed);
    }
    mpz_vm_sys::reveal(is_valid(&puzzle, &sol)).wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Wikipedia example puzzle and its unique solution.
    const PUZZLE_STR: &str =
        "530070000600195000098000060800060003400803001700020006060000280000419005000080079";
    const SOLUTION_STR: &str =
        "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

    fn grid(s: &str) -> [u8; GRID] {
        let mut g = [0u8; GRID];
        for (cell, ch) in g.iter_mut().zip(s.bytes()) {
            *cell = ch - b'0';
        }
        g
    }

    #[test]
    fn accepts_the_solution() {
        assert_eq!(is_valid(&grid(PUZZLE_STR), &grid(SOLUTION_STR)), 1);
    }

    #[test]
    fn rejects_a_tampered_cell() {
        let mut sol = grid(SOLUTION_STR);
        sol[0] = 4; // duplicates within row/col/box
        assert_eq!(is_valid(&grid(PUZZLE_STR), &sol), 0);
    }

    #[test]
    fn rejects_a_solution_of_a_different_puzzle() {
        // Internally consistent grid that ignores the clues: shift every
        // cell's value by one (1..=9 cycle) — units stay permutations.
        let mut sol = grid(SOLUTION_STR);
        for c in sol.iter_mut() {
            *c = *c % 9 + 1;
        }
        assert_eq!(is_valid(&grid(PUZZLE_STR), &sol), 0);
    }

    #[test]
    fn rejects_out_of_range_cells() {
        let mut sol = grid(SOLUTION_STR);
        sol[80] = 0;
        assert_eq!(is_valid(&grid(PUZZLE_STR), &sol), 0);
    }
}
