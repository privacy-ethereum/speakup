//! A Speakup demo guest: aggregate statistics over private values.
//!
//! Proves that the **average** of a private list of numbers reaches a
//! **public** threshold — without revealing any value, the sum, or even the
//! exact average. Think "our team's mean salary is at least X" or "my
//! sensor readings averaged within spec".
//!
//! The values live in [`VALUES`] as little-endian `u32`s, private to the
//! prover; the count and threshold are public parameters. The guest sums
//! the (symbolic) values — reassembling each from its bytes with
//! constant-amount shifts — and reveals a single 0/1:
//! `sum >= threshold * n` (the right-hand side is public × public, hence
//! concrete). Mean vs. threshold as a product avoids dividing a symbolic
//! sum.
//!
//! Host-enforced bounds keep everything inside `i32`: at most
//! [`MAX_VALUES`] values, each at most [`MAX_VALUE`], threshold at most
//! [`MAX_VALUE`].

use core::sync::atomic::{AtomicU8, Ordering};

/// Maximum number of values.
pub const MAX_VALUES: usize = 64;
/// Maximum magnitude of each value and of the threshold; keeps
/// `sum` and `threshold * n` comfortably inside `i32`.
pub const MAX_VALUE: i32 = 1_000_000;

/// The private values, each a little-endian `u32`.
static VALUES: [AtomicU8; MAX_VALUES * 4] = [const { AtomicU8::new(0) }; MAX_VALUES * 4];

/// Address of the private values buffer.
#[no_mangle]
pub extern "C" fn values_ptr() -> i32 {
    VALUES.as_ptr() as i32
}

/// Whether the mean of `values` is at least `threshold`; branch-free in the
/// (private) values.
fn mean_at_least_inner(values: &[i32], threshold: i32) -> i32 {
    let mut sum = 0i32;
    for &v in values {
        sum += v;
        sum = core::hint::black_box(sum);
    }
    // mean >= threshold  <=>  sum >= threshold * n; the right side is
    // public so the multiply is concrete.
    (sum >= threshold * values.len() as i32) as i32
}

/// Loads `n` private values, compares their mean against the public
/// `threshold`, and reveals only the 0/1 flag.
#[no_mangle]
pub extern "C" fn mean_at_least(n: i32, threshold: i32) -> i32 {
    let n = (n as u32 as usize).min(MAX_VALUES);
    let mut values = [0i32; MAX_VALUES];
    for (i, v) in values[..n].iter_mut().enumerate() {
        let mut x = 0i32;
        for k in 0..4 {
            x |= (VALUES[i * 4 + k].load(Ordering::Relaxed) as i32) << (8 * k);
        }
        *v = x;
    }
    mpz_vm_sys::reveal(mean_at_least_inner(&values[..n], threshold)).wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_the_mean() {
        // mean = 63_666.66…
        let salaries = [62_000, 71_000, 58_000];
        assert_eq!(mean_at_least_inner(&salaries, 60_000), 1);
        assert_eq!(mean_at_least_inner(&salaries, 63_666), 1);
        assert_eq!(mean_at_least_inner(&salaries, 63_667), 0);
        assert_eq!(mean_at_least_inner(&salaries, 70_000), 0);
    }

    #[test]
    fn exact_boundary() {
        assert_eq!(mean_at_least_inner(&[10, 20], 15), 1);
        assert_eq!(mean_at_least_inner(&[10, 20], 16), 0);
    }
}
