//! Browser smoke tests: the zk-vm Prover/Verifier pair runs end-to-end in
//! headless Chrome over the real OT stack. Run with
//! `wasm-pack test --headless --chrome --release -- --features no-bundler`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
use zkvm_demo::{
    age_zkvm, build_table, csv_zkvm, custom_zkvm, guest_wasm, json_info, json_zkvm, luhn_zkvm,
    module_exports, prover_square, regex_zkvm, sha256_zkvm, square_zkvm, transcript_info,
    transcript_zkvm, verifier_square,
};

// A dedicated worker, not the page: rayon's parallel sections block the
// calling thread (`Atomics.wait`), which the main browser thread forbids.
wasm_bindgen_test_configure!(run_in_dedicated_worker);

/// Starts web-spawn + the rayon pool (idempotent across tests).
async fn init_threads() {
    zkvm_demo::initialize(4).await.expect("threading init");
}

#[wasm_bindgen_test]
async fn luhn_runs_in_browser() {
    init_threads().await;
    // A standard Visa test number; the second has a one-digit typo.
    assert_eq!(luhn_zkvm("4539148803436467".into()).await.unwrap(), 1);
    assert_eq!(luhn_zkvm("4539148803436468".into()).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn csv_runs_in_browser() {
    init_threads().await;
    // The whole CSV is private; column 0's mean is 63'666.66…
    let csv = "62000,12\n71000,8\n58000,15\n";
    assert_eq!(csv_zkvm(csv.into(), 0, 60_000).await.unwrap(), 1);
    assert_eq!(csv_zkvm(csv.into(), 0, 70_000).await.unwrap(), 0);
    // A malformed document proves nothing.
    assert_eq!(csv_zkvm("62a00,12\n".into(), 0, 0).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn square_runs_over_a_message_channel() {
    init_threads().await;
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
    init_threads().await;
    // (6 + 1)² = 49, with 6 as the prover's private input.
    let out = square_zkvm(6).await.expect("zk-vm run should succeed");
    assert_eq!(out, 49);
}

#[wasm_bindgen_test]
async fn age_runs_in_browser() {
    init_threads().await;
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
    init_threads().await;
    // Private "alice@example.com" matches the public email pattern.
    let pat = r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}";
    assert_eq!(regex_zkvm(pat.into(), "alice@example.com".into()).await.unwrap(), 1);
    assert_eq!(regex_zkvm(pat.into(), "not-an-email".into()).await.unwrap(), 0);
}

#[wasm_bindgen_test]
async fn custom_module_runs_in_browser() {
    init_threads().await;
    // The "drop your own .wasm" path, fed one of our own guests: square's
    // compute(x) with x private.
    let wasm = guest_wasm("square").unwrap();
    let exports = module_exports(&wasm).unwrap();
    assert!(
        exports.contains(r#""name":"compute","params":["i32"],"results":["i32"],"supported":true"#),
        "unexpected exports: {exports}"
    );
    let out = custom_zkvm(wasm, "compute".into(), vec![1], vec![6]).await.unwrap();
    assert_eq!(out, "49");
}

#[wasm_bindgen_test]
async fn json_runs_in_browser() {
    init_threads().await;
    // The swiss-bank shape from tlsn-extension's demo fixture.
    let doc = r#"{"bank":"Swiss Bank","account_id":"ETH-042","accounts":{"CHF":"50_000_000"}}"#;
    // Assert mode: prove the CHF balance without revealing the document.
    let out = json_zkvm(doc.into(), "accounts.CHF".into(), Some("50_000_000".into()))
        .await
        .unwrap();
    assert_eq!(out, r#"{"ok":1,"value":""}"#);
    // A wrong expected value encodes fine but legitimately fails to prove.
    let out = json_zkvm(doc.into(), "accounts.CHF".into(), Some("90_000_000".into()))
        .await
        .unwrap();
    assert_eq!(out, r#"{"ok":0,"value":""}"#);
    // Disclose mode: reveal the value at the path.
    let out = json_zkvm(doc.into(), "account_id".into(), None).await.unwrap();
    assert_eq!(out, r#"{"ok":1,"value":"ETH-042"}"#);
    // The info JSON drives the page's path dropdown.
    let info = json_info(doc.into()).unwrap();
    assert!(info.contains(r#""path":"accounts.CHF""#), "{info}");
    // Bad inputs fail before the protocol runs.
    assert!(json_zkvm(doc.into(), "no.such.path".into(), None).await.is_err());
    assert!(json_zkvm("not json".into(), "".into(), None).await.is_err());
}

#[wasm_bindgen_test]
async fn transcript_runs_in_browser() {
    init_threads().await;
    // Assert mode: prove the API assigned id 101 (transcript-verify's
    // jsonplaceholder_post fixture) — only the 0/1 flag is revealed.
    let out = transcript_zkvm("id".into(), Some("101".into())).await.unwrap();
    assert_eq!(out, r#"{"ok":1,"value":""}"#);
    // A wrong expected value encodes fine but legitimately fails to prove.
    let out = transcript_zkvm("id".into(), Some("102".into())).await.unwrap();
    assert_eq!(out, r#"{"ok":0,"value":""}"#);
    // Disclose mode still works: reveal the value at the path.
    let out = transcript_zkvm("title".into(), None).await.unwrap();
    assert_eq!(out, r#"{"ok":1,"value":"foo"}"#);
    // The info JSON drives the page's path dropdown and claim line.
    let info = transcript_info().unwrap();
    assert!(info.contains(r#""path":"id""#), "{info}");
    assert!(info.contains(r#""host":"jsonplaceholder.typicode.com""#), "{info}");
    assert!(info.contains(r#""reqBody":true"#), "{info}");
    assert!(info.contains(r#""status":201"#), "{info}");
    // Paths that don't resolve to a scalar fail before the protocol runs.
    assert!(transcript_zkvm("no.such.path".into(), None).await.is_err());
    // Header claims: disclose one header from each side over the real
    // protocol (the request-header value is masked out of `sent`).
    assert!(info.contains(r#""respHeaders":["#), "{info}");
    let out = transcript_zkvm("resp:0".into(), None).await.unwrap();
    assert!(out.starts_with(r#"{"ok":1,"value":""#), "{out}");
    let out = transcript_zkvm("req:0".into(), None).await.unwrap();
    assert!(out.starts_with(r#"{"ok":1,"value":""#), "{out}");
    // An out-of-range header index fails before the protocol runs.
    assert!(transcript_zkvm("resp:99".into(), None).await.is_err());
}

#[wasm_bindgen_test]
async fn sha256_runs_in_browser() {
    init_threads().await;
    // SHA-256("abc") is the classic test vector.
    let digest = sha256_zkvm(b"abc".to_vec()).await.unwrap();
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[wasm_bindgen_test]
async fn sha256_handles_a_1kib_message() {
    init_threads().await;
    // Past the old 4 KiB-buffer layout's assumptions: the digest now lands
    // at ptr + len. Reference digest of 1024 x 0xAB from hashlib.
    let digest = sha256_zkvm(vec![0xAB; 1024]).await.unwrap();
    assert_eq!(
        digest,
        "4555555dc68d872c2270ba89ecc5f6f094812f65372b37e50071fe5168031c49"
    );
}
