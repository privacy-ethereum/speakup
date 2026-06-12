//! The JSON-only flat table: selective disclosure over a private JSON
//! document, without the HTTP layers of the transcript format.
//!
//! Same advice pattern as the transcript table — the host parses the
//! document OFF the VM, the resulting (public) node table drives the
//! guest's cursor, and every private byte is re-checked at its claimed
//! position, branch-free — with the HTTP claims dropped: this table
//! carries only the node spans, one JSON path, and the assert/disclose
//! claim. The byte-level JSON language accepted is identical to the
//! transcript format's response body — both run
//! [`verify_json_claim`](crate::verify_json_claim) — so the demo cuts
//! listed in the crate docs (ASCII strings, raw dup-key compare, the size
//! caps) apply here unchanged.
//!
//! The host side reuses the real upstream parser: [`synth_exchange`] wraps
//! the document in a minimal Content-Length-framed exchange so
//! `transcript_verify::parse_transcript` (feature `parse`, host-only)
//! emits the node table — raw == decoded under that framing, so the spans
//! are already in document coordinates.

use transcript_verify::{JsonKind, JsonNode};

use crate::encode::{kind_code, parse_path, resolve_path};
use crate::{
    Disclosure, JsonClaim, MODE_ASSERT, MODE_DISCLOSE, OUT_CAP, TABLE_CAP_WORDS, f, get_node, gw,
    verify_json_claim,
};

/// JSON-only flat-table format version (independent of the transcript
/// table's [`crate::VERSION`]).
pub const VERSION: u32 = 1;

/// Maximum private document size.
pub const DOC_CAP: usize = 4096;

// === flat-table header word indices ===

pub const W_VERSION: usize = 0;
pub const W_DOC_LEN: usize = 1;
pub const W_N_NODES: usize = 2;
pub const W_NODES_OFF: usize = 3;
pub const W_N_PATH: usize = 4;
/// Must be 0 when the path has no components (the claim is the root).
pub const W_PATH_OFF: usize = 5;
pub const W_VALUE_NODE: usize = 6;
pub const W_STR_OFF: usize = 7;
pub const W_STR_BYTES: usize = 8;
/// 0 = disclose the value's bytes; 1 = assert they equal the public
/// expected string (nothing revealed but the 0/1 flag).
pub const W_CLAIM_MODE: usize = 9;
/// The expected-value string (assert mode; must be 0/0 in disclose mode).
pub const W_EXPECT_SOFF: usize = 10;
pub const W_EXPECT_SLEN: usize = 11;
/// Number of fixed header words; the variable sections follow.
pub const HEADER_WORDS: usize = 12;

/// The (public) disclosed-value content span recorded in a flat table:
/// `(start, len)` in document coordinates — `(0, 0)` in assert mode, where
/// nothing is revealed but the flag. Both hosts use it to size the readback
/// of the guest's revealed output buffer; [`verify`] re-derives and fully
/// checks the same span in the VM.
pub fn disclosure_span(table: &[u32]) -> Result<(usize, usize), &'static str> {
    if gw(table, W_CLAIM_MODE)? == MODE_ASSERT {
        return Ok((0, 0));
    }
    let nodes_off = gw(table, W_NODES_OFF)?;
    let n_nodes = gw(table, W_N_NODES)?;
    let v = gw(table, W_VALUE_NODE)?;
    if v >= n_nodes {
        return Err("value node out of range");
    }
    let nd = get_node(table, nodes_off, v)?;
    if nd.end < nd.start || nd.end - nd.start > OUT_CAP {
        return Err("value span invalid");
    }
    Ok((nd.start, nd.end - nd.start))
}

/// Verifies the private document bytes against the public flat `table`.
/// Concrete `Err` = malformed table (both parties reject it identically,
/// outside the private domain); otherwise the returned [`Disclosure::ok`]
/// is the symbolic conjunction of every byte check.
pub fn verify(doc: &[u8], table: &[u32]) -> Result<Disclosure, &'static str> {
    let mut bad = 0i32;

    if gw(table, W_VERSION)? != VERSION as usize {
        return Err("unsupported table version");
    }
    if gw(table, W_DOC_LEN)? != doc.len() {
        return Err("table/buffer length mismatch");
    }
    if doc.is_empty() {
        return Err("empty document");
    }
    if doc.len() > DOC_CAP || table.len() > TABLE_CAP_WORDS {
        return Err("input too large");
    }
    let str_off = gw(table, W_STR_OFF)?;
    let str_bytes = gw(table, W_STR_BYTES)?;
    if str_off + str_bytes.div_ceil(4) > table.len() {
        return Err("string region out of bounds");
    }
    let n_nodes = gw(table, W_N_NODES)?;
    if n_nodes == 0 {
        return Err("the document must be claimed as JSON");
    }
    let jc = JsonClaim {
        n_nodes,
        nodes_off: gw(table, W_NODES_OFF)?,
        n_path: gw(table, W_N_PATH)?,
        path_off: gw(table, W_PATH_OFF)?,
        value_node: gw(table, W_VALUE_NODE)?,
        str_off,
        str_bytes,
        mode: gw(table, W_CLAIM_MODE)?,
        expect_soff: gw(table, W_EXPECT_SOFF)?,
        expect_slen: gw(table, W_EXPECT_SLEN)?,
    };
    let (value_start, value_len) = verify_json_claim(doc, 0, doc.len(), table, &jc, &mut bad)?;

    Ok(Disclosure {
        ok: f(bad == 0),
        value_start,
        value_len,
        // There is only the one document buffer.
        value_in_sent: false,
    })
}

/// Wraps a document in a minimal HTTP exchange so the REAL upstream host
/// parser (`transcript_verify::parse_transcript`, feature `parse`) can be
/// reused to emit the JSON node table: the response body is exactly `doc`
/// under Content-Length framing (raw == decoded), so the emitted spans are
/// already in document coordinates. Host-side only — the exchange never
/// enters the VM and nothing about it is claimed.
pub fn synth_exchange(doc: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let sent = b"GET /document HTTP/1.1\r\nHost: json.invalid\r\n\r\n".to_vec();
    let mut recv = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        doc.len()
    )
    .into_bytes();
    recv.extend_from_slice(doc);
    (sent, recv)
}

/// The encoded public inputs for one JSON-document claim.
#[derive(Debug)]
pub struct Encoded {
    /// The flat table, ready to be written into the guest as public data.
    pub words: Vec<u32>,
    /// Content span start of the disclosed value, in document coordinates.
    pub value_start: usize,
    /// Content length of the disclosed value.
    pub value_len: usize,
}

/// Encodes the claim "`doc` is a well-formed JSON document with a value at
/// `path`" as the guest's flat public table. `nodes` is the upstream node
/// table for `doc` (document coordinates — see [`synth_exchange`]). With
/// `expect = None` the value's bytes are disclosed; with `Some(v)` the
/// guest instead asserts they equal `v` and reveals only the 0/1 flag (a
/// mismatched `v` encodes fine — the proof then legitimately fails).
pub fn encode(
    doc: &[u8],
    nodes: &[JsonNode],
    path: &str,
    expect: Option<&str>,
) -> Result<Encoded, String> {
    if doc.is_empty() {
        return Err("the document is empty".into());
    }
    if doc.len() > DOC_CAP {
        return Err(format!("document larger than {DOC_CAP} bytes"));
    }
    // Demo cut: the verifier checks JSON strings as ASCII, not UTF-8.
    if doc.iter().any(|&b| b >= 0x80) {
        return Err("non-ASCII JSON documents are out of scope for this demo".into());
    }
    if nodes.is_empty() {
        return Err("empty node table".into());
    }

    let comps = parse_path(path)?;
    let (resolved, cur) = resolve_path(nodes, doc, &comps)?;
    let value = &nodes[cur];
    match value.kind {
        JsonKind::Null | JsonKind::Bool | JsonKind::Number | JsonKind::String => {}
        _ => return Err("the path must point to a scalar (string, number, bool, null)".into()),
    }
    let value_start = value.start as usize;
    let value_len = (value.end - value.start) as usize;
    if value_len > OUT_CAP {
        return Err(format!("the value is larger than {OUT_CAP} bytes"));
    }

    // === assemble the words ===
    let nodes_off = HEADER_WORDS;
    let path_off = nodes_off + 4 * nodes.len();
    let str_off = path_off + 4 * resolved.len();

    // Packed string region: key components ++ expected value.
    let mut strs: Vec<u8> = Vec::new();
    let mut put_str = |bytes: &[u8]| -> (u32, u32) {
        let off = strs.len() as u32;
        strs.extend_from_slice(bytes);
        (off, bytes.len() as u32)
    };
    let comp_strs: Vec<(u32, u32)> = resolved.iter().map(|(_, _, k)| put_str(k)).collect();
    // Assert mode: the public expected value; pinned to 0/0 when disclosing.
    let (expect_soff, expect_slen) = match expect {
        Some(e) => put_str(e.as_bytes()),
        None => (0, 0),
    };

    let total = str_off + strs.len().div_ceil(4);
    if total > TABLE_CAP_WORDS {
        return Err(format!("table needs {total} words (cap {TABLE_CAP_WORDS})"));
    }

    let mut w = vec![0u32; total];
    w[W_VERSION] = VERSION;
    w[W_DOC_LEN] = doc.len() as u32;
    w[W_N_NODES] = nodes.len() as u32;
    w[W_NODES_OFF] = nodes_off as u32;
    w[W_N_PATH] = resolved.len() as u32;
    // Pinned to 0 when there are no components (the verifier requires it,
    // so the word has no slack when it is meaningless).
    w[W_PATH_OFF] = if resolved.is_empty() { 0 } else { path_off as u32 };
    w[W_VALUE_NODE] = cur as u32;
    w[W_STR_OFF] = str_off as u32;
    w[W_STR_BYTES] = strs.len() as u32;
    w[W_CLAIM_MODE] = if expect.is_some() {
        MODE_ASSERT as u32
    } else {
        MODE_DISCLOSE as u32
    };
    w[W_EXPECT_SOFF] = expect_soff;
    w[W_EXPECT_SLEN] = expect_slen;

    for (i, n) in nodes.iter().enumerate() {
        w[nodes_off + 4 * i] = kind_code(n.kind);
        w[nodes_off + 4 * i + 1] = n.start;
        w[nodes_off + 4 * i + 2] = n.end;
        w[nodes_off + 4 * i + 3] = n.size;
    }

    for (i, ((is_index, idx, _), &(soff, slen))) in
        resolved.iter().zip(comp_strs.iter()).enumerate()
    {
        w[path_off + 4 * i] = *is_index;
        w[path_off + 4 * i + 1] = *idx;
        w[path_off + 4 * i + 2] = soff;
        w[path_off + 4 * i + 3] = slen;
    }

    for (j, &b) in strs.iter().enumerate() {
        w[str_off + j / 4] |= (b as u32) << ((j % 4) * 8);
    }

    // Self-check: the flat verifier must accept its own encoding — except
    // that an assert against a non-matching expected value must (only)
    // fail the byte checks.
    let value_bytes = &doc[value_start..value_start + value_len];
    let should_hold = expect.is_none_or(|e| e.as_bytes() == value_bytes);
    let (want_start, want_len) = if expect.is_some() {
        (0, 0)
    } else {
        (value_start, value_len)
    };
    match verify(doc, &w) {
        Ok(d)
            if d.ok == should_hold as i32
                && d.value_start == want_start
                && d.value_len == want_len => {}
        Ok(_) => return Err("internal: flat encoding failed its byte checks".into()),
        Err(e) => return Err(format!("internal: flat encoding rejected: {e}")),
    }

    Ok(Encoded {
        words: w,
        value_start,
        value_len,
    })
}
