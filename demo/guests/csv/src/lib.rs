//! A Speakup demo guest: parse a CSV inside the VM and average one column.
//!
//! The **whole CSV document is private**; the column index and the threshold
//! are public. The guest scans the private bytes once, branch-free, doing
//! real parsing work obliviously:
//!
//!   - tracks the current column (commas increment it, newlines reset it) —
//!     positions of the delimiters are private, so the column counter is a
//!     symbolic value updated with mask arithmetic;
//!   - builds each cell's number digit by digit (`acc = acc·10 + d`, masked
//!     to digit bytes);
//!   - when a cell ends (comma or newline) *and* its column equals the
//!     public target, adds it into the running sum — `sum += acc & -(take)`,
//!     a select without a select;
//!   - counts rows (newlines) and validates the document as it goes: only
//!     digits/commas/newlines, no empty cells, cells at most 5 digits,
//!     every row reaches the target column, and the document ends at a row
//!     boundary.
//!
//! Revealed: a single 0/1 — "the document is well-formed CSV and the mean
//! of the target column is at least the threshold". The verifier learns the
//! byte length, never the contents, the row count, or the sum (the mean
//! comparison multiplies the *symbolic* row count by the public threshold,
//! so the count itself is never disclosed).
//!
//! Bounds (host-enforced): documents up to [`CSV_CAP`] bytes, cells up to 5
//! digits (≤ 99,999), threshold ≤ 99,999 — sums and products stay well
//! inside `i32`.
//!
//! Format: digit-only cells separated by `,`, rows terminated by `\n`
//! (including the last row); the host normalizes away spaces and `\r`.

use core::sync::atomic::{AtomicU8, Ordering};

/// Maximum document size.
pub const CSV_CAP: usize = 8192;
/// Maximum target column index.
pub const MAX_COL: i32 = 16;
/// Maximum cell value / threshold (5 digits).
pub const MAX_CELL: i32 = 99_999;

/// The private CSV document.
static CSV: [AtomicU8; CSV_CAP] = [const { AtomicU8::new(0) }; CSV_CAP];

/// Address of the private CSV buffer.
#[no_mangle]
pub extern "C" fn csv_ptr() -> i32 {
    CSV.as_ptr() as i32
}

/// Whether `bytes` is well-formed CSV whose column `col_k` has a mean of at
/// least `threshold`; branch-free in the (private) bytes.
fn csv_mean_at_least(bytes: &[u8], col_k: i32, threshold: i32) -> i32 {
    let bb = core::hint::black_box::<i32>;

    let mut col = 0i32; // current column in this row
    let mut acc = 0i32; // current cell value
    let mut run = 0i32; // current cell's digit count
    let mut sum = 0i32; // sum over the target column
    let mut count = 0i32; // rows seen
    let mut bad = 0i32; // nonzero once anything is malformed

    for &byte in bytes {
        let b = byte as i32;
        // Every flag is pinned with black_box *at creation*: LLVM proves
        // bare flags are 0/1 and lowers `0 - flag` and friends into
        // (symbolic) selects, which the VM rejects.
        let is_comma = bb((b == b',' as i32) as i32);
        let is_nl = bb((b == b'\n' as i32) as i32);
        let is_digit = bb(((b >= b'0' as i32) as i32) & ((b <= b'9' as i32) as i32));
        let is_sep = is_comma | is_nl;

        // Only digits, commas, and newlines are allowed.
        bad |= 1 - (is_sep | is_digit);

        // A separator ends a cell: harvest it if it sits in the target
        // column; it must not be empty; a newline must have reached the
        // target column and ends the row.
        let in_target = bb((col == col_k) as i32);
        let take = bb(is_sep & in_target);
        sum += acc & 0i32.wrapping_sub(take);
        bad |= is_sep & bb((run == 0) as i32);
        bad |= is_nl & bb((col < col_k) as i32);
        count += is_nl;

        // Build the cell value over digit runs; reset on separators.
        let dmask = 0i32.wrapping_sub(is_digit);
        acc = (acc * 10 + (b - b'0' as i32)) & dmask;
        run = (run + 1) & dmask;
        bad |= bb((run > 5) as i32); // cells <= 5 digits keep sums in i32

        // Commas advance the column; newlines reset it for the next row.
        let keep_row = 0i32.wrapping_sub(1 - is_nl);
        col = (col + is_comma) & keep_row;

        acc = bb(acc);
        run = bb(run);
        col = bb(col);
        sum = bb(sum);
        count = bb(count);
        bad = bb(bad);
    }

    // The document must end exactly at a row boundary (trailing newline).
    bad |= ((col != 0) as i32) | ((run != 0) as i32);

    let valid = bb(((bad == 0) as i32) & ((count > 0) as i32));
    // mean >= threshold  <=>  sum >= threshold * count. `count` is symbolic
    // (the verifier never learns how many rows there were); the threshold
    // is public.
    let ge = bb((sum >= threshold * count) as i32);
    valid & ge
}

/// Loads `len` private bytes, parses the CSV obliviously, and reveals only
/// the 0/1 "column mean reaches the threshold" flag.
#[no_mangle]
pub extern "C" fn mean_at_least(len: i32, col: i32, threshold: i32) -> i32 {
    let len = (len as u32 as usize).min(CSV_CAP);
    let mut bytes = [0u8; CSV_CAP];
    for (dst, slot) in bytes[..len].iter_mut().zip(CSV.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    mpz_vm_sys::reveal(csv_mean_at_least(&bytes[..len], col, threshold)).wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALARIES: &str = "62000,12\n71000,8\n58000,15\n90000,4\n";

    #[test]
    fn averages_a_column() {
        // mean(col 0) = 70'250; mean(col 1) = 9.75.
        assert_eq!(csv_mean_at_least(SALARIES.as_bytes(), 0, 60_000), 1);
        assert_eq!(csv_mean_at_least(SALARIES.as_bytes(), 0, 70_250), 1);
        assert_eq!(csv_mean_at_least(SALARIES.as_bytes(), 0, 70_251), 0);
        assert_eq!(csv_mean_at_least(SALARIES.as_bytes(), 1, 9), 1);
        assert_eq!(csv_mean_at_least(SALARIES.as_bytes(), 1, 10), 0);
    }

    #[test]
    fn single_cell_rows() {
        assert_eq!(csv_mean_at_least(b"5\n7\n", 0, 6), 1);
        assert_eq!(csv_mean_at_least(b"5\n7\n", 0, 7), 0);
    }

    #[test]
    fn rejects_malformed_documents() {
        // Non-digit garbage, empty cells, missing trailing newline, a row
        // without the target column, an empty document, oversized cells.
        assert_eq!(csv_mean_at_least(b"62a00\n", 0, 0), 0);
        assert_eq!(csv_mean_at_least(b"1,,3\n", 0, 0), 0);
        assert_eq!(csv_mean_at_least(b"1,2", 0, 0), 0);
        assert_eq!(csv_mean_at_least(b"1,2\n3\n", 1, 0), 0);
        assert_eq!(csv_mean_at_least(b"", 0, 0), 0);
        assert_eq!(csv_mean_at_least(b"123456\n", 0, 0), 0);
    }

    #[test]
    fn ragged_extra_columns_are_fine() {
        // Rows may have *more* columns than the target; only fewer is an
        // error (the target cell must exist in every row).
        assert_eq!(csv_mean_at_least(b"1,2,3\n4,5\n", 1, 3), 1);
    }
}
