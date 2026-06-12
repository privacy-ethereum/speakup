//! A Speakup demo guest: prove a private card number is well-formed.
//!
//! Checks the Luhn checksum (the check digit scheme of payment card
//! numbers, IMEIs, etc.) over a **private** digit string. Only the 0/1
//! "checksum valid" flag is revealed — the number itself never is.
//!
//! A nice contrast with the regex guest: for a fixed length the Luhn
//! language is regular in principle, but its DFA has hundreds of states —
//! while as arithmetic it's a handful of additions. Different tools for
//! different proofs.
//!
//! Branch-free discipline, as in the sibling guests: the doubling rule
//! `d -> 2d - 9·[2d > 9]` uses a comparison flag (pinned with `black_box`
//! so LLVM can't lower it back into a `select`), digit-range validation is
//! OR-accumulated, and the final `sum % 10 == 0` is computed by comparing
//! against the (few, public) multiples of ten rather than a division.

use core::sync::atomic::{AtomicU8, Ordering};

/// Longest supported number (ISO/IEC 7812 allows up to 19 digits).
pub const MAX_LEN: usize = 19;

/// The private number, as ASCII digit bytes.
static NUMBER: [AtomicU8; MAX_LEN] = [const { AtomicU8::new(0) }; MAX_LEN];

/// Address of the private number buffer.
#[no_mangle]
pub extern "C" fn number_ptr() -> i32 {
    NUMBER.as_ptr() as i32
}

/// Whether `digits` (ASCII) passes the Luhn check; branch-free in the
/// (private) bytes.
fn luhn_valid(digits: &[u8]) -> i32 {
    let n = digits.len();
    let mut bad = 0u32;
    let mut sum = 0i32;
    for (i, &byte) in digits.iter().enumerate() {
        let b = byte as i32;
        // Every byte must be an ASCII digit.
        bad |= ((b < b'0' as i32) as u32) | ((b > b'9' as i32) as u32);
        let d = b - b'0' as i32;

        // Double every second digit from the right; the position parity is
        // public, so this `if` is concrete control flow.
        let val = if (n - 1 - i) % 2 == 1 {
            let dd = d * 2;
            // dd - 9 if dd > 9: the flag is pinned so the arithmetic can't
            // be lowered back into a (symbolic) select.
            let carry = core::hint::black_box((dd > 9) as i32);
            dd - 9 * carry
        } else {
            d
        };
        sum += val;
        sum = core::hint::black_box(sum);
    }

    // sum <= 9 * MAX_LEN = 171, so `sum % 10 == 0` is a small disjunction
    // of equalities — no division on symbolic values needed.
    let mut is_mult = 0u32;
    let mut m = 0i32;
    while m <= 9 * MAX_LEN as i32 {
        is_mult |= (sum == m) as u32;
        m += 10;
    }

    (core::hint::black_box(is_mult) & ((core::hint::black_box(bad) == 0) as u32)) as i32
}

/// Loads `len` private digit bytes, checks the Luhn sum, and reveals only
/// the 0/1 validity flag.
#[no_mangle]
pub extern "C" fn check(len: i32) -> i32 {
    let len = (len as u32 as usize).min(MAX_LEN);
    let mut digits = [0u8; MAX_LEN];
    for (dst, slot) in digits[..len].iter_mut().zip(NUMBER.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    mpz_vm_sys::reveal(luhn_valid(&digits[..len])).wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_numbers() {
        // Standard test numbers (Visa, Mastercard, Amex).
        assert_eq!(luhn_valid(b"4539148803436467"), 1);
        assert_eq!(luhn_valid(b"5555555555554444"), 1);
        assert_eq!(luhn_valid(b"378282246310005"), 1);
    }

    #[test]
    fn rejects_a_typo() {
        // Same as the Visa number with one digit changed and two swapped —
        // exactly the errors Luhn is designed to catch.
        assert_eq!(luhn_valid(b"4539148803436468"), 0);
        assert_eq!(luhn_valid(b"4539148803436476"), 0);
    }

    #[test]
    fn rejects_non_digits() {
        assert_eq!(luhn_valid(b"4539x48803436467"), 0);
    }
}
