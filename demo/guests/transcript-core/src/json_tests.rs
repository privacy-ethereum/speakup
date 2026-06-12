//! Native tests for the JSON-only table (`crate::json`): parse documents
//! with the REAL upstream parser (via the synthetic-exchange wrapper),
//! encode, and check that the branch-free verifier accepts — then tamper
//! with bytes and table words and check rejection.
//!
//! The shared JSON grammar checks themselves (numbers, escapes, gaps,
//! headers-free walk) are exercised exhaustively by `tests.rs` through the
//! transcript table; both formats run the same `verify_json_claim`. These
//! tests pin the JSON-only header layout and the differential property on
//! documents alone.

use transcript_verify::parse_transcript;

use crate::json::{
    Encoded, W_PATH_OFF, W_STR_BYTES, disclosure_span, encode, synth_exchange, verify,
};

/// The demo's default document shape (the swiss-bank fixture from
/// tlsn-extension), extended with the remaining scalar kinds, escapes,
/// arrays, and an empty string.
fn sample() -> Vec<u8> {
    concat!(
        "{\n",
        "    \"bank\": \"Swiss Bank\",\n",
        "    \"account_id\": \"ETH-042\",\n",
        "    \"accounts\": {\"EUR\": \"275_000_000\", \"CHF\": \"50_000_000\"},\n",
        "    \"id\": 132, \"alive\": true, \"gone\": null, \"w\": -1.5e2,\n",
        "    \"tags\": [\"a\\\"b\", \"c\", [0, {}]],\n",
        "    \"empty\": \"\"\n",
        "}"
    )
    .as_bytes()
    .to_vec()
}

/// The upstream node table for `doc`, via the synthetic-exchange wrapper.
fn parse_doc(doc: &[u8]) -> Vec<transcript_verify::JsonNode> {
    let (sent, recv) = synth_exchange(doc);
    let table = parse_transcript(&sent, &recv).expect("synthetic exchange parses");
    table
        .response
        .body
        .expect("response has a body")
        .json
        .expect("body parses as JSON")
        .nodes
}

fn encode_doc(doc: &[u8], path: &str, expect: Option<&str>) -> Encoded {
    encode(doc, &parse_doc(doc), path, expect).expect("encodes")
}

fn ok_flag(doc: &[u8], words: &[u32]) -> i32 {
    verify(doc, words).expect("table shape is valid").ok
}

fn flat_accepts(doc: &[u8], words: &[u32]) -> bool {
    matches!(verify(doc, words), Ok(d) if d.ok == 1)
}

/// Whether upstream `validate` accepts the mutated document under the
/// original document's table (same length, so the synthetic head is
/// byte-identical).
fn upstream_accepts(orig: &[u8], mutated: &[u8]) -> bool {
    assert_eq!(orig.len(), mutated.len());
    let (sent, orecv) = synth_exchange(orig);
    let table = parse_transcript(&sent, &orecv).unwrap();
    let (_, mrecv) = synth_exchange(mutated);
    transcript_verify::validate(&sent, &mrecv, &table).is_ok()
}

#[test]
fn accepts_the_sample_and_discloses_each_scalar_kind() {
    let doc = sample();
    for (path, want) in [
        ("bank", "Swiss Bank"),
        ("account_id", "ETH-042"),
        ("accounts.CHF", "50_000_000"),
        ("id", "132"),
        ("alive", "true"),
        ("gone", "null"),
        ("w", "-1.5e2"),
        ("tags.0", "a\\\"b"), // raw content: escapes undecoded
        ("tags.2.0", "0"),
        ("empty", ""),
    ] {
        let enc = encode_doc(&doc, path, None);
        let d = verify(&doc, &enc.words).expect("valid table");
        assert_eq!(d.ok, 1, "path {path}");
        assert_eq!(
            &doc[d.value_start..d.value_start + d.value_len],
            want.as_bytes(),
            "path {path}"
        );
        assert_eq!(
            disclosure_span(&enc.words).unwrap(),
            (d.value_start, d.value_len),
            "path {path}"
        );
    }
}

#[test]
fn scalar_roots_take_the_empty_path() {
    for (doc, want) in [
        (&b"42"[..], "42"),
        (b" \"hi\" ", "hi"),
        (b"true", "true"),
        (b"null", "null"),
    ] {
        let enc = encode_doc(doc, "", None);
        let d = verify(doc, &enc.words).expect("valid table");
        assert_eq!(d.ok, 1, "{doc:?}");
        assert_eq!(&doc[d.value_start..d.value_start + d.value_len], want.as_bytes());
    }
    // Assert mode on a scalar root.
    let enc = encode_doc(b"42", "", Some("42"));
    assert_eq!(ok_flag(b"42", &enc.words), 1);
    let enc = encode_doc(b"42", "", Some("43"));
    assert_eq!(ok_flag(b"42", &enc.words), 0);
    // An object root is not a scalar: the empty path must not encode.
    let doc = sample();
    assert!(encode(&doc, &parse_doc(&doc), "", None).is_err());
}

#[test]
fn byte_flips_never_widen_the_accept_set() {
    // For every single-byte flip of the document: if upstream rejects the
    // mutation under the original table, the flat verifier must reject
    // too. (The converse doesn't hold — the flat table also pins the
    // claimed path's key bytes.)
    let doc = sample();
    let enc = encode_doc(&doc, "accounts.CHF", None);
    for i in 0..doc.len() {
        let mut m = doc.clone();
        m[i] ^= 0x01;
        if flat_accepts(&m, &enc.words) {
            assert!(
                upstream_accepts(&doc, &m),
                "flat accepted a flip at doc[{i}] ({:?}) that upstream rejects",
                doc[i] as char
            );
        }
    }
}

#[test]
fn claim_key_bytes_are_pinned() {
    // Renaming a key ON the claimed path keeps the document well-formed
    // (upstream accepts the new parse... of the same spans), but breaks
    // the claim: flat must reject.
    let doc = sample();
    let enc = encode_doc(&doc, "accounts.CHF", None);
    for pat in ["\"accounts\"", "\"CHF\""] {
        let pos = doc.windows(pat.len()).position(|w| w == pat.as_bytes()).unwrap();
        let mut m = doc.clone();
        m[pos + 1] = b'X';
        assert!(upstream_accepts(&doc, &m), "{pat}");
        assert!(!flat_accepts(&m, &enc.words), "{pat} rename accepted");
    }
}

#[test]
fn unclaimed_values_stay_free() {
    // Flipping bytes inside values OFF the claimed path keeps the proof
    // valid: the statement covers the structure and the claim, not the
    // hidden bytes.
    let doc = sample();
    let enc = encode_doc(&doc, "accounts.CHF", None);
    for pat in ["Swiss Bank", "ETH-042", "275_000_000"] {
        let pos = doc.windows(pat.len()).position(|w| w == pat.as_bytes()).unwrap();
        let mut m = doc.clone();
        m[pos] = b'x';
        assert_eq!(ok_flag(&m, &enc.words), 1, "{pat}");
    }
}

#[test]
fn assert_mode_pins_the_value() {
    let doc = sample();
    // Correct expected value: proven, nothing disclosed.
    let enc = encode_doc(&doc, "accounts.CHF", Some("50_000_000"));
    let d = verify(&doc, &enc.words).unwrap();
    assert_eq!((d.ok, d.value_start, d.value_len), (1, 0, 0));
    assert_eq!(disclosure_span(&enc.words).unwrap(), (0, 0));
    // Same length, wrong bytes: encodes fine, proof fails.
    let enc = encode_doc(&doc, "accounts.CHF", Some("90_000_000"));
    assert_eq!(ok_flag(&doc, &enc.words), 0);
    // Different length: encodes fine, proof fails.
    let enc = encode_doc(&doc, "accounts.CHF", Some("1"));
    assert_eq!(ok_flag(&doc, &enc.words), 0);
    // Unlike disclose mode, the value's bytes are now PINNED.
    let enc = encode_doc(&doc, "accounts.CHF", Some("50_000_000"));
    let pos = doc.windows(10).position(|w| w == b"50_000_000").unwrap();
    let mut m = doc.clone();
    m[pos] = b'9';
    assert_eq!(ok_flag(&m, &enc.words), 0);
}

#[test]
fn rejects_duplicate_keys() {
    let doc = b"{\"a\":1,\"b\":2}".to_vec();
    let enc = encode_doc(&doc, "a", None);
    // Rename key "b" to "a" in the bytes: a raw duplicate.
    let pos = doc.windows(3).position(|w| w == b"\"b\"").unwrap();
    let mut m = doc.clone();
    m[pos + 1] = b'a';
    assert!(!flat_accepts(&m, &enc.words));
    assert!(!upstream_accepts(&doc, &m));
}

#[test]
fn rejects_tampered_table_words() {
    // Any single-word increment must reject: either the shape check trips,
    // or a byte/claim check fails. (No word of the table is slack.)
    let doc = sample();
    let tables = [
        encode_doc(&doc, "accounts.CHF", None).words,
        encode_doc(&doc, "accounts.CHF", Some("50_000_000")).words,
    ];
    for (t, words) in tables.iter().enumerate() {
        for i in 0..words.len() {
            // The string region's byte count has sub-word slack: bumping it
            // by one within the same padded word references only unused
            // padding.
            if i == W_STR_BYTES {
                continue;
            }
            let mut w = words.clone();
            w[i] = w[i].wrapping_add(1);
            assert!(
                !flat_accepts(&doc, &w),
                "table {t} word {i} += 1 accepted (value {})",
                words[i]
            );
        }
    }
    // The scalar-root table has an empty path and an empty string region,
    // so even W_PATH_OFF and W_STR_BYTES are pinned: no word has slack.
    let doc = b"42".to_vec();
    let words = encode_doc(&doc, "", None).words;
    assert_eq!(words[W_PATH_OFF], 0);
    for i in 0..words.len() {
        let mut w = words.clone();
        w[i] = w[i].wrapping_add(1);
        assert!(
            !flat_accepts(&doc, &w),
            "root table word {i} += 1 accepted (value {})",
            words[i]
        );
    }
}

#[test]
fn rejects_out_of_scope_documents() {
    let doc = sample();
    let nodes = parse_doc(&doc);
    // Unknown and non-scalar paths.
    assert!(encode(&doc, &nodes, "nope", None).is_err());
    assert!(encode(&doc, &nodes, "accounts", None).is_err()); // object
    assert!(encode(&doc, &nodes, "tags", None).is_err()); // array
    assert!(encode(&doc, &nodes, "tags.9", None).is_err());
    assert!(encode(&doc, &nodes, "bank.x", None).is_err()); // through a string
    // Non-ASCII (upstream allows UTF-8; the demo cut doesn't).
    let ndoc = "{\"a\":\"é\"}".as_bytes().to_vec();
    let nnodes = parse_doc(&ndoc);
    assert!(encode(&ndoc, &nnodes, "a", None).unwrap_err().contains("ASCII"));
    // Not JSON at all: the upstream parser claims the body opaque.
    let (sent, recv) = synth_exchange(b"{\"a\":}");
    let table = parse_transcript(&sent, &recv).unwrap();
    assert!(table.response.body.unwrap().json.is_none());
}
