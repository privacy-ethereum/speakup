//! A minimal demo guest for the zk-vm: squares a private input.
//!
//! Exports a single function, [`compute`], that takes an integer `x` — which
//! the prover supplies privately — computes `(x + 1)^2`, and reveals the result
//! through the VCI so the verifier learns it too. The reveal is two-phase:
//! [`mpz_vm_sys::reveal`] returns a handle immediately, and `wait` blocks for
//! the now-public value.
//!
//! Built natively, the `mpz-vm-sys` bindings are clear-execution no-ops, so the
//! same code runs and can be unit-tested off the VM.

/// Computes `(x + 1)^2` over a (possibly private) `x`, reveals it, and returns
/// the now-public result.
#[no_mangle]
pub extern "C" fn compute(x: i32) -> i32 {
    let y = (x + 1) * (x + 1);
    mpz_vm_sys::reveal(y).wait()
}

#[cfg(test)]
mod tests {
    use super::compute;

    // Built natively, `reveal(y).wait()` is a no-op that hands `y` straight
    // back, so `compute` is just `(x + 1)^2` and can be checked directly.
    #[test]
    fn squares_x_plus_one() {
        assert_eq!(compute(6), 49);
        assert_eq!(compute(0), 1);
        assert_eq!(compute(-1), 0);
        assert_eq!(compute(10), 121);
    }

    #[test]
    fn matches_reference_over_a_range() {
        for x in -100..=100 {
            assert_eq!(compute(x), (x + 1) * (x + 1));
        }
    }
}
