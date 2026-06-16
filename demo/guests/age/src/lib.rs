//! A zk-vm demo guest: privacy-preserving age verification.
//!
//! This is the zk-vm analog of TLSNotary's `examples-zk` age check. The prover
//! holds a birth date privately and proves they are 18 or older without
//! disclosing the date itself. Here the date is a private string written into
//! guest memory; the only thing revealed through the VCI is a single boolean.
//!
//! The flow is:
//!
//!   1. The host writes a `"YYYY-MM-DD"` birth date into [`BIRTHDATE`] —
//!      private to the prover, blind to the verifier — using the address
//!      returned by [`birthdate_ptr`].
//!   2. The host calls [`is_adult`] with today's date packed as `YYYYMMDD`.
//!   3. The guest parses the (symbolic) birth date, compares against `today`,
//!      and reveals only the `0/1` adult flag.
//!
//! The parse is deliberately **branch-free**: it never makes a control-flow
//! decision based on the private bytes (which the VM cannot resolve locally),
//! only arithmetic and a final comparison whose symbolic result is revealed.
//!
//! The buffer is `[AtomicU8; N]` rather than `static mut`: the host writes its
//! bytes out-of-band (through `Vm::write`), so the guest must read them with no
//! `unsafe` and without letting the optimizer assume they are still zero — both
//! of which atomic loads give us. `AtomicU8` has the same one-byte layout as
//! `u8`, so the host's write lands exactly where the guest reads.
//!
//! Built natively, the `mpz-vm-sys` bindings are clear-execution no-ops, so the
//! same code runs and can be unit-tested off the VM.

use core::sync::atomic::{AtomicU8, Ordering};

/// Length of the birth-date buffer; fits `"YYYY-MM-DD"` (10 bytes).
const DATE_LEN: usize = 10;

/// Reserved buffer the host fills with the private `"YYYY-MM-DD"` birth date.
static BIRTHDATE: [AtomicU8; DATE_LEN] = [const { AtomicU8::new(0) }; DATE_LEN];

/// Returns the address of the [`BIRTHDATE`] buffer so the host knows where to
/// write the private date. The address is public — only the bytes are private.
#[no_mangle]
pub extern "C" fn birthdate_ptr() -> i32 {
    BIRTHDATE.as_ptr() as i32
}

/// Whether a `"YYYY-MM-DD"` birth `date` makes the holder 18 or older as of
/// `today` (packed `YYYYMMDD`). Returns `1` if 18+, else `0`.
///
/// The parse is **branch-free**: each ASCII byte becomes a digit via
/// subtraction, and the only decision is the final comparison — never a branch
/// on the (private) date bytes. Packed `YYYYMMDD` integers order the same as
/// calendar dates, so "18 or older" is exactly `today >= birth + 18 years`,
/// i.e. a difference of at least `18_0000` in packed form.
fn age_flag(date: &[u8; DATE_LEN], today: i32) -> i32 {
    let d = |i: usize| (date[i] as i32) - (b'0' as i32);

    // "YYYY-MM-DD": digits at 0..4, 5..7, 8..10; '-' at 4 and 7.
    let year = d(0) * 1000 + d(1) * 100 + d(2) * 10 + d(3);
    let month = d(5) * 10 + d(6);
    let day = d(8) * 10 + d(9);
    let birth = year * 10000 + month * 100 + day;

    // 18 years in packed `YYYYMMDD` form is +18 in the `YYYY` place: 18 * 10000.
    (today >= birth + 18 * 10000) as i32
}

/// Loads the host-written birth date from [`BIRTHDATE`], computes the adult
/// flag against `today` (packed `YYYYMMDD`), and reveals it through the VCI.
/// Returns the now-public flag: `1` if 18+, else `0`.
#[no_mangle]
pub extern "C" fn is_adult(today: i32) -> i32 {
    let mut date = [0u8; DATE_LEN];
    for (dst, slot) in date.iter_mut().zip(BIRTHDATE.iter()) {
        *dst = slot.load(Ordering::Relaxed);
    }
    mpz_vm_sys::reveal(age_flag(&date, today)).wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `"YYYY-MM-DD"` as the fixed-size byte array `age_flag` expects.
    fn date(s: &str) -> [u8; DATE_LEN] {
        s.as_bytes().try_into().expect("date must be YYYY-MM-DD")
    }

    /// A calendar date packed as `YYYYMMDD`, the form `age_flag` takes `today`
    /// in.
    fn ymd(year: i32, month: i32, day: i32) -> i32 {
        year * 10000 + month * 100 + day
    }

    // `age_flag` is the whole age check; the atomic-loading glue in `is_adult`
    // is exercised end-to-end by `crates/vm-zk/tests/age.rs`.
    #[test]
    fn detects_adult() {
        // Comfortably over 18 by 2024.
        assert_eq!(age_flag(&date("1985-03-12"), ymd(2024, 6, 10)), 1);
    }

    #[test]
    fn detects_minor() {
        assert_eq!(age_flag(&date("2010-01-01"), ymd(2024, 6, 10)), 0);
    }

    #[test]
    fn boundary_on_eighteenth_birthday() {
        // The day before the 18th birthday is still a minor; the birthday
        // itself is an adult.
        assert_eq!(age_flag(&date("2006-06-10"), ymd(2024, 6, 9)), 0);
        assert_eq!(age_flag(&date("2006-06-10"), ymd(2024, 6, 10)), 1);
    }
}
