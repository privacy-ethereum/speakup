//! Browser smoke tests: the zk-vm Prover/Verifier pair runs end-to-end in
//! headless Chrome over the real OT stack. Run with
//! `wasm-pack test --headless --chrome`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
use zkvm_demo::{
    age_zkvm, build_table, csv_zkvm, luhn_zkvm, prover_square, regex_zkvm, sha256_zkvm,
    square_zkvm, verifier_square, wat_zkvm,
};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn luhn_runs_in_browser() {
    // A standard Visa test number; the second has a one-digit typo.
    assert_eq!(luhn_zkvm("4539148803436467".into()).await.unwrap(), 1);
    assert_eq!(luhn_zkvm("4539148803436468".into()).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn csv_runs_in_browser() {
    // The whole CSV is private; column 0's mean is 63'666.66…
    let csv = "62000,12\n71000,8\n58000,15\n";
    assert_eq!(csv_zkvm(csv.into(), 0, 60_000).await.unwrap(), 1);
    assert_eq!(csv_zkvm(csv.into(), 0, 70_000).await.unwrap(), 0);
    // A malformed document proves nothing.
    assert_eq!(csv_zkvm("62a00,12\n".into(), 0, 0).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn square_runs_over_a_message_channel() {
    // The per-party entry points over a real MessageChannel — the same
    // transport the two-worker app uses, here with both ends in this test.
    let chan = web_sys::MessageChannel::new().unwrap();
    let (p, v) = futures::future::join(
        prover_square(chan.port1(), 6),
        verifier_square(chan.port2()),
    )
    .await;
    assert_eq!(p.ok(), Some(49));
    assert_eq!(v.ok(), Some(49));
}

#[wasm_bindgen_test]
async fn square_runs_in_browser() {
    // (6 + 1)² = 49, with 6 as the prover's private input.
    let out = square_zkvm(6).await.expect("zk-vm run should succeed");
    assert_eq!(out, 49);
}

#[wasm_bindgen_test]
async fn age_runs_in_browser() {
    // Born 1985-03-12, checked as of 2026-06-10: 18+.
    let adult = age_zkvm("1985-03-12".into(), 2026_06_10).await.unwrap();
    assert_eq!(adult, 1);
    // Born 2010-01-01: a minor.
    let minor = age_zkvm("2010-01-01".into(), 2026_06_10).await.unwrap();
    assert_eq!(minor, 0);
}

#[wasm_bindgen_test]
fn regex_table_matches_off_vm() {
    // The table builder and the guest's matcher, exercised without the
    // protocol: fast coverage of the DFA serialization.
    let cases = [
        (r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}", "alice@example.com", true),
        (r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}", "not an email", false),
        (r"a+b", "aab", true),
        (r"a+b", "xaab", false), // anchored: whole string must match
        (r"a+b", "aabx", false),
        (r"(ha)+!", "hahaha!", true),
        (r"(ha)+!", "hahahe!", false),
    ];
    for (pattern, text, expected) in cases {
        let table = build_table(pattern).expect("table should build");
        let words = regex_dfa_core::decode_table(&table);
        assert_eq!(
            regex_dfa_core::dfa_matches(&words, text.as_bytes()) == 1,
            expected,
            "{pattern} vs {text}"
        );
    }
}

#[wasm_bindgen_test]
async fn regex_runs_in_browser() {
    // Private "alice@example.com" matches the public email pattern.
    let pat = r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}";
    assert_eq!(regex_zkvm(pat.into(), "alice@example.com".into()).await.unwrap(), 1);
    assert_eq!(regex_zkvm(pat.into(), "not-an-email".into()).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn wat_runs_in_browser() {
    // The custom-guest template: x*x over a private x, revealed.
    let src = r#"(module
  (import "vc" "reveal_i32" (func $reveal (param i32) (result i32)))
  (import "vc" "reveal_i32_wait" (func $wait (param i32) (result i32)))
  (func (export "compute") (param $x i32) (result i32)
    (call $wait (call $reveal (i32.mul (local.get $x) (local.get $x))))))"#;
    assert_eq!(wat_zkvm(src.into(), 7).await.unwrap(), 49);
}

#[wasm_bindgen_test]
async fn sha256_runs_in_browser() {
    // SHA-256("abc") is the classic test vector.
    let digest = sha256_zkvm(b"abc".to_vec()).await.unwrap();
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[wasm_bindgen_test]
async fn sha256_handles_a_1kib_message() {
    // Past the old 4 KiB-buffer layout's assumptions: the digest now lands
    // at ptr + len. Reference digest of 1024 x 0xAB from hashlib.
    let digest = sha256_zkvm(vec![0xAB; 1024]).await.unwrap();
    assert_eq!(
        digest,
        "4555555dc68d872c2270ba89ecc5f6f094812f65372b37e50071fe5168031c49"
    );
}
