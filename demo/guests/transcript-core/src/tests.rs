//! Native tests: synthesize exchanges, parse them with the REAL upstream
//! parser (`transcript_verify::parse_transcript`), encode, and check that
//! the branch-free verifier accepts — then tamper with bytes and table
//! words and check rejection.
//!
//! The key differential property: the flat verifier proves "upstream-valid
//! transcript AND the claims hold", so its accept set must be a SUBSET of
//! upstream's for every byte mutation (never accept what upstream rejects),
//! and must stay accepting where only unclaimed private bytes change.

use transcript_verify::parse_transcript;

use crate::encode::{Encoded, encode};
use crate::{W_RESP_CL_IDX, W_STATUS, W_VALUE_NODE, verify};

/// A small but structurally rich exchange: nested objects/arrays, escapes,
/// numbers, bools, null, whitespace, an empty header value.
fn sample() -> (Vec<u8>, Vec<u8>) {
    let body = concat!(
        "{\"name\":\"ditto\", \"id\":132, \"alive\":true, \"gone\":null,\n",
        "  \"w\":-1.5e2, \"tags\":[\"a\\\"b\", \"c\"], \"nest\":{\"deep\":[0, {}]},\n",
        "  \"empty\":\"\"}"
    );
    let sent = b"GET /pets/132?full=1 HTTP/1.1\r\n\
        Host: api.example\r\n\
        Accept: */*\r\n\
        X-Empty:\r\n\
        X-Padded:   spaced out  \r\n\
        \r\n"
        .to_vec();
    let recv = format!(
        "HTTP/1.1 200 OK\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        \r\n\
        {}",
        body.len(),
        body
    )
    .into_bytes();
    (sent, recv)
}

fn encode_sample(path: &str) -> (Vec<u8>, Vec<u8>, Encoded) {
    let (sent, recv) = sample();
    let table = parse_transcript(&sent, &recv).expect("fixture parses");
    let enc = encode(&sent, &recv, &table, path, None).expect("fixture encodes");
    (sent, recv, enc)
}

/// The accepted flag of `verify` on a valid-shaped table.
fn ok_flag(sent: &[u8], recv: &[u8], words: &[u32]) -> i32 {
    verify(sent, recv, words).expect("table shape is valid").ok
}

/// Whether the flat verifier accepts (shape valid AND every byte check).
fn flat_accepts(sent: &[u8], recv: &[u8], words: &[u32]) -> bool {
    matches!(verify(sent, recv, words), Ok(d) if d.ok == 1)
}

/// Whether upstream `validate` accepts the bytes under the sample's table.
fn upstream_accepts(sent: &[u8], recv: &[u8]) -> bool {
    let (osent, orecv) = sample();
    let table = parse_transcript(&osent, &orecv).unwrap();
    transcript_verify::validate(sent, recv, &table).is_ok()
}

#[test]
fn accepts_the_sample_and_discloses_each_scalar_kind() {
    for (path, want) in [
        ("name", "ditto"),
        ("id", "132"),
        ("alive", "true"),
        ("gone", "null"),
        ("w", "-1.5e2"),
        ("tags.0", "a\\\"b"), // raw content: escapes undecoded
        ("tags.1", "c"),
        ("nest.deep.0", "0"),
        ("empty", ""),
    ] {
        let (sent, recv, enc) = encode_sample(path);
        let d = verify(&sent, &recv, &enc.words).expect("valid table");
        assert_eq!(d.ok, 1, "path {path}");
        assert_eq!(
            &recv[d.value_start..d.value_start + d.value_len],
            want.as_bytes(),
            "path {path}"
        );
        assert_eq!(enc.status, 200);
    }
}

#[test]
fn rejects_unknown_paths_and_non_scalars() {
    let (sent, recv) = sample();
    let table = parse_transcript(&sent, &recv).unwrap();
    assert!(encode(&sent, &recv, &table, "nope", None).is_err());
    assert!(encode(&sent, &recv, &table, "tags.7", None).is_err());
    assert!(encode(&sent, &recv, &table, "tags", None).is_err()); // array
    assert!(encode(&sent, &recv, &table, "nest", None).is_err()); // object
    assert!(encode(&sent, &recv, &table, "name.x", None).is_err()); // through a string
}

#[test]
fn byte_flips_never_widen_the_accept_set() {
    // For every single-byte flip in either buffer: if upstream rejects the
    // mutated transcript under the original table, the flat verifier must
    // reject too. (The converse doesn't hold — the flat verifier also pins
    // the claims: method, target, Host, status.)
    let (sent, recv, enc) = encode_sample("name");
    for i in 0..sent.len() {
        let mut s = sent.clone();
        s[i] ^= 0x01;
        if flat_accepts(&s, &recv, &enc.words) {
            assert!(
                upstream_accepts(&s, &recv),
                "flat accepted a flip at sent[{i}] ({:?}) that upstream rejects",
                sent[i] as char
            );
        }
    }
    for i in 0..recv.len() {
        let mut r = recv.clone();
        r[i] ^= 0x01;
        if flat_accepts(&sent, &r, &enc.words) {
            assert!(
                upstream_accepts(&sent, &r),
                "flat accepted a flip at recv[{i}] ({:?}) that upstream rejects",
                recv[i] as char
            );
        }
    }
}

#[test]
fn claim_bytes_are_pinned() {
    let (sent, recv, enc) = encode_sample("name");
    // Method, target, and Host value flips keep the transcript well-formed
    // (upstream accepts), but break the claim: flat must reject.
    for pat in ["GET", "/pets/132?full=1", "api.example"] {
        let pos = sent
            .windows(pat.len())
            .position(|w| w == pat.as_bytes())
            .unwrap();
        let mut s = sent.clone();
        s[pos] ^= 0x01;
        assert!(upstream_accepts(&s, &recv), "{pat}");
        assert!(!flat_accepts(&s, &recv, &enc.words), "{pat} flip accepted");
    }
    // Status digit flip: 200 -> 201.
    let mut r = recv.clone();
    r[11] ^= 0x01;
    assert!(upstream_accepts(&sent, &r));
    assert!(!flat_accepts(&sent, &r, &enc.words));
    // The claimed path's KEY bytes are pinned too.
    let pos = recv.windows(6).position(|w| w == b"\"name\"").unwrap();
    let mut r = recv.clone();
    r[pos + 1] = b'g';
    assert!(upstream_accepts(&sent, &r));
    assert!(!flat_accepts(&sent, &r, &enc.words));
}

#[test]
fn unclaimed_private_bytes_stay_free() {
    // Flipping a byte inside an unclaimed header value (Accept) or an
    // undisclosed JSON value keeps the proof valid: the statement covers
    // the claims, not the hidden bytes.
    let (sent, recv, enc) = encode_sample("name");
    let pos = sent.windows(3).position(|w| w == b"*/*").unwrap();
    let mut s = sent.clone();
    s[pos] = b'x';
    assert_eq!(ok_flag(&s, &recv, &enc.words), 1);
    // "132" -> "172" (the id is not the disclosed path).
    let pos = recv.windows(8).position(|w| w == b"\"id\":132").unwrap();
    let mut r = recv.clone();
    r[pos + 6] = b'7';
    assert_eq!(ok_flag(&sent, &r, &enc.words), 1);
}

#[test]
fn rejects_wrong_claims() {
    let (sent, recv, enc) = encode_sample("name");
    // Wrong status claim.
    let mut w = enc.words.clone();
    w[W_STATUS] = 201;
    assert!(!flat_accepts(&sent, &recv, &w));
    // Content-Length claimed at the wrong header.
    let mut w = enc.words.clone();
    w[W_RESP_CL_IDX] = 0; // Content-Type
    assert!(!flat_accepts(&sent, &recv, &w));
    // A different (valid, scalar) node claimed as the path's value.
    let (_, _, other) = encode_sample("id");
    let mut w = enc.words.clone();
    w[W_VALUE_NODE] = other.words[W_VALUE_NODE];
    assert!(!flat_accepts(&sent, &recv, &w));
}

#[test]
fn rejects_renamed_content_length_twin() {
    // Craft a response with a second 14-byte header and rename it to
    // Content-Length in the BYTES only: two CL-named lines must be
    // rejected (framing ambiguity).
    let (sent, _) = sample();
    let body = "{\"a\":1}";
    let recv = format!(
        "HTTP/1.1 200 OK\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        X-Duplicate-Cl: {}\r\n\
        \r\n\
        {}",
        body.len(),
        body.len(),
        body
    )
    .into_bytes();
    let table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "a", None).unwrap();
    assert_eq!(ok_flag(&sent, &recv, &enc.words), 1);
    let pos = recv
        .windows(14)
        .position(|w| w == b"X-Duplicate-Cl")
        .unwrap();
    let mut r = recv.clone();
    r[pos..pos + 14].copy_from_slice(b"Content-Length");
    assert!(!flat_accepts(&sent, &r, &enc.words));
    assert!(transcript_verify::validate(&sent, &r, &table).is_err());
}

#[test]
fn rejects_duplicate_keys() {
    let (sent, _) = sample();
    let body = "{\"a\":1,\"b\":2}";
    let recv = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes();
    let table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "a", None).unwrap();
    // Rename key "b" to "a" in the bytes: a raw duplicate.
    let pos = recv.windows(3).position(|w| w == b"\"b\"").unwrap();
    let mut r = recv.clone();
    r[pos + 1] = b'a';
    assert!(!flat_accepts(&sent, &r, &enc.words));
    assert!(transcript_verify::validate(&sent, &r, &table).is_err());
}

#[test]
fn rejects_tampered_table_words() {
    let (sent, recv, enc) = encode_sample("name");
    // Any single-word increment must reject: either the shape check trips,
    // or a byte/claim check fails. (No word of the table is slack.)
    for i in 0..enc.words.len() {
        // The string region's byte count has sub-word slack: bumping it by
        // one within the same padded word references only unused padding.
        if i == crate::W_STR_BYTES {
            continue;
        }
        let mut w = enc.words.clone();
        w[i] = w[i].wrapping_add(1);
        assert!(
            !flat_accepts(&sent, &recv, &w),
            "table word {i} += 1 accepted (value {})",
            enc.words[i]
        );
    }
}

#[test]
fn accepts_no_reason_and_empty_reason_forms() {
    let (sent, _) = sample();
    for status_line in ["HTTP/1.1 200\r\n", "HTTP/1.1 200 \r\n"] {
        let body = "{\"a\":true}";
        let recv = format!(
            "{}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            status_line,
            body.len(),
            body
        )
        .into_bytes();
        let table = parse_transcript(&sent, &recv).unwrap();
        let enc = encode(&sent, &recv, &table, "a", None).unwrap();
        assert_eq!(ok_flag(&sent, &recv, &enc.words), 1, "{status_line:?}");
    }
}

#[test]
fn rejects_out_of_scope_transcripts() {
    let (sent, recv) = sample();
    // Chunked framing.
    let chunked: &[u8] =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n";
    let table = parse_transcript(&sent, chunked).unwrap();
    assert!(
        encode(&sent, chunked, &table, "a", None)
            .unwrap_err()
            .contains("framing")
    );
    // A chunked request body (request bodies are otherwise in scope).
    let csent: &[u8] =
        b"POST /x HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n0\r\n\r\n";
    let table = parse_transcript(csent, &recv).unwrap();
    assert!(
        encode(csent, &recv, &table, "name", None)
            .unwrap_err()
            .contains("framing")
    );
    // Non-ASCII body.
    let body = "{\"a\":\"é\"}";
    let nrecv = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes();
    let table = parse_transcript(&sent, &nrecv).unwrap();
    assert!(
        encode(&sent, &nrecv, &table, "a", None)
            .unwrap_err()
            .contains("ASCII")
    );
}

#[test]
fn number_grammar_is_exact() {
    let (sent, _) = sample();
    for lit in ["0", "-0", "12.5", "1e9", "-1.5E+10", "1.5e-3"] {
        let body = format!("{{\"n\":{lit}}}");
        let recv = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .into_bytes();
        let table = parse_transcript(&sent, &recv).unwrap();
        let enc = encode(&sent, &recv, &table, "n", None).unwrap();
        assert_eq!(ok_flag(&sent, &recv, &enc.words), 1, "{lit}");
        // In-place corruptions of the lexeme must reject.
        let pos = recv
            .windows(lit.len() + 1)
            .position(|w| w == format!(":{lit}").as_bytes())
            .unwrap()
            + 1;
        for (from, to) in [(b'1', b'+'), (b'5', b'.'), (b'e', b','), (b'0', b'/')] {
            if let Some(at) = recv[pos..pos + lit.len()].iter().position(|&b| b == from) {
                let mut r = recv.clone();
                r[pos + at] = to;
                assert!(
                    !flat_accepts(&sent, &r, &enc.words),
                    "{lit}: {} -> {} accepted",
                    from as char,
                    to as char
                );
            }
        }
    }
}

#[test]
fn string_escape_grammar_is_exact() {
    let (sent, _) = sample();
    // Raw string: the body literally contains A and \n escapes.
    let body = r#"{"s":"\u0041 x\n ok"}"#;
    let recv = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes();
    let table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "s", None).unwrap();
    assert_eq!(ok_flag(&sent, &recv, &enc.words), 1);
    // Break the \u escape's hex.
    let pos = recv.windows(2).position(|w| w == b"\\u").unwrap();
    let mut r = recv.clone();
    r[pos + 2] = b'g';
    assert!(!flat_accepts(&sent, &r, &enc.words));
    // Turn a valid escape char into an invalid one.
    let npos = recv.windows(2).position(|w| w == b"\\n").unwrap();
    let mut r = recv.clone();
    r[npos + 1] = b'q';
    assert!(!flat_accepts(&sent, &r, &enc.words));
}

/// A POST with a JSON request body (covered opaquely) and a JSON response —
/// the jsonplaceholder_post shape.
fn post_sample() -> (Vec<u8>, Vec<u8>) {
    let req_body = r#"{"title":"foo","body":"bar","userId":1}"#;
    let sent = format!(
        "POST /posts HTTP/1.1\r\n\
        Host: api.example\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        \r\n\
        {}",
        req_body.len(),
        req_body
    )
    .into_bytes();
    let body = "{\n  \"title\": \"foo\",\n  \"userId\": 1,\n  \"id\": 101\n}";
    let recv = format!(
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes();
    (sent, recv)
}

#[test]
fn accepts_a_request_body_opaquely() {
    let (sent, recv) = post_sample();
    let table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "id", None).unwrap();
    let d = verify(&sent, &recv, &enc.words).unwrap();
    assert_eq!(d.ok, 1);
    assert_eq!(&recv[d.value_start..d.value_start + d.value_len], b"101");
    assert_eq!(enc.status, 201);
    // The request body is unconstrained: flipping a byte in it changes
    // nothing the statement covers.
    let pos = sent.windows(5).position(|w| w == b"\"foo\"").unwrap();
    let mut s = sent.clone();
    s[pos + 1] = b'g';
    assert_eq!(ok_flag(&s, &recv, &enc.words), 1);
    // But the framing is pinned: truncating via the CL digits is caught.
    let pos = sent.windows(18).position(|w| w == b"Content-Length: 39").unwrap();
    let mut s = sent.clone();
    s[pos + 17] = b'8';
    assert!(!flat_accepts(&s, &recv, &enc.words));
}

#[test]
fn assert_mode_pins_the_value() {
    let (sent, recv) = post_sample();
    let table = parse_transcript(&sent, &recv).unwrap();
    // Correct expected value: proven, nothing disclosed.
    let enc = encode(&sent, &recv, &table, "id", Some("101")).unwrap();
    let d = verify(&sent, &recv, &enc.words).unwrap();
    assert_eq!((d.ok, d.value_start, d.value_len), (1, 0, 0));
    assert_eq!(crate::disclosure_span(&enc.words).unwrap(), (0, 0));
    // Same length, wrong bytes: encodes fine, proof fails.
    let enc = encode(&sent, &recv, &table, "id", Some("102")).unwrap();
    assert_eq!(ok_flag(&sent, &recv, &enc.words), 0);
    // Different length: encodes fine, proof fails.
    let enc = encode(&sent, &recv, &table, "id", Some("1014")).unwrap();
    assert_eq!(ok_flag(&sent, &recv, &enc.words), 0);
    // Unlike disclose mode, the value's bytes are now PINNED: tampering
    // them in the transcript breaks the assert.
    let enc = encode(&sent, &recv, &table, "id", Some("101")).unwrap();
    let pos = recv.windows(9).position(|w| w == b"\"id\": 101").unwrap();
    let mut r = recv.clone();
    r[pos + 8] = b'2';
    assert_eq!(ok_flag(&sent, &r, &enc.words), 0);
    // String values assert too.
    let enc = encode(&sent, &recv, &table, "title", Some("foo")).unwrap();
    assert_eq!(ok_flag(&sent, &recv, &enc.words), 1);
}

#[test]
fn post_byte_flips_never_widen_the_accept_set() {
    // Same differential as the GET sample, against upstream validate with
    // the request body claimed OPAQUE (the legal claim the flat table
    // encodes; the parsed table claims it as JSON, which checks MORE).
    let (sent, recv) = post_sample();
    let mut table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "id", None).unwrap();
    table.request.body.as_mut().unwrap().json = None;
    let upstream =
        |s: &[u8], r: &[u8]| transcript_verify::validate(s, r, &table).is_ok();
    for i in 0..sent.len() {
        let mut s = sent.clone();
        s[i] ^= 0x01;
        if flat_accepts(&s, &recv, &enc.words) {
            assert!(upstream(&s, &recv), "flip at sent[{i}] ({:?})", sent[i] as char);
        }
    }
    for i in 0..recv.len() {
        let mut r = recv.clone();
        r[i] ^= 0x01;
        if flat_accepts(&sent, &r, &enc.words) {
            assert!(upstream(&sent, &r), "flip at recv[{i}] ({:?})", recv[i] as char);
        }
    }
}

#[test]
fn post_rejects_tampered_table_words() {
    // The no-slack property, on a table with a request body AND an assert
    // claim (exercising W_REQ_CL_IDX, W_CLAIM_MODE, W_EXPECT_*).
    let (sent, recv) = post_sample();
    let table = parse_transcript(&sent, &recv).unwrap();
    let enc = encode(&sent, &recv, &table, "id", Some("101")).unwrap();
    for i in 0..enc.words.len() {
        if i == crate::W_STR_BYTES {
            continue; // sub-word padding slack, as in the GET test
        }
        let mut w = enc.words.clone();
        w[i] = w[i].wrapping_add(1);
        assert!(
            !flat_accepts(&sent, &recv, &w),
            "table word {i} += 1 accepted (value {})",
            enc.words[i]
        );
    }
}
