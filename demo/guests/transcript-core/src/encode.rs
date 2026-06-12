//! Host-side encoder: a validated `transcript_verify::SpanTable` plus a
//! claim → the flat public `u32` table the guest verifies.
//!
//! Runs OFF the VM (in the prover's worker, at native wasm speed). The
//! encoder first runs the real upstream validator — so no doomed table ever
//! reaches the guest — then resolves the claim (a JSON path to node
//! indices, or a header line to its record), extracts the public claims
//! (method, target, `Host` value, status), assembles the words, and finally
//! cross-checks its own output against [`crate::verify`] natively.

use transcript_verify::{Framing, HeaderSpan, JsonKind, JsonNode, SpanTable, validate};

use crate::{
    HEADER_WORDS, MAX_HEADERS, MAX_PATH, MODE_ASSERT, MODE_DISCLOSE, OUT_CAP, RECV_CAP, SENT_CAP,
    TABLE_CAP_WORDS, TARGET_JSON, TARGET_REQ_HEADER, TARGET_RESP_HEADER, VERSION, W_CLAIM_MODE,
    W_CLAIM_TARGET, W_EXPECT_SLEN, W_EXPECT_SOFF, W_HDR_IDX, W_HDR_NAME_SLEN, W_HDR_NAME_SOFF,
    W_HOST_SLEN, W_HOST_SOFF, W_METHOD_END, W_METHOD_SLEN, W_METHOD_SOFF, W_N_NODES, W_N_PATH,
    W_NODES_OFF, W_PATH_OFF, W_REASON_END, W_REASON_START, W_RECV_LEN, W_REQ_CL_IDX,
    W_REQ_HDRS_OFF, W_REQ_HEAD_END, W_REQ_HOST_IDX, W_REQ_N_HDRS, W_RESP_CL_IDX, W_RESP_HDRS_OFF,
    W_RESP_HEAD_END, W_RESP_N_HDRS, W_SENT_LEN, W_STATUS, W_STR_BYTES, W_STR_OFF, W_TARGET_END,
    W_TARGET_SLEN, W_TARGET_SOFF, W_VALUE_NODE, W_VERSION,
};

/// The encoded public inputs for one transcript claim.
#[derive(Debug)]
pub struct Encoded {
    /// The flat table, ready to be written into the guest as public data.
    pub words: Vec<u32>,
    /// The verified status code.
    pub status: u16,
    /// Content span of the disclosed value, in `recv` coordinates (`sent`
    /// coordinates for a request-header claim).
    pub value_start: usize,
    /// Content length of the disclosed value.
    pub value_len: usize,
}

/// What one flat table claims about the exchange, beyond the always-pinned
/// method, target, `Host`, and status.
pub enum Claim<'a> {
    /// A scalar value of the response's JSON body, at a spansy-style dot
    /// path; the whole body is grammar-checked.
    Json { path: &'a str },
    /// One request header line, named publicly. The response body is then
    /// covered opaquely (framing pinned, contents unconstrained), so
    /// non-JSON exchanges are in scope.
    RequestHeader { index: usize },
    /// One response header line, named publicly; response body covered
    /// opaquely as above.
    ResponseHeader { index: usize },
}

/// One parsed path component (spansy-style dot path: decimal components are
/// array indices, everything else is an object key).
pub(crate) enum Comp<'a> {
    Key(&'a [u8]),
    Index(usize),
}

pub(crate) fn parse_path(path: &str) -> Result<Vec<Comp<'_>>, String> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let comps: Vec<Comp> = path
        .split('.')
        .map(|c| {
            if !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()) {
                Comp::Index(c.parse().unwrap_or(usize::MAX))
            } else {
                Comp::Key(c.as_bytes())
            }
        })
        .collect();
    if comps.len() > MAX_PATH {
        return Err(format!("path has more than {MAX_PATH} components"));
    }
    if comps.iter().any(|c| matches!(c, Comp::Key(k) if k.is_empty() || k.len() > 64)) {
        return Err("path components must be 1..=64 bytes".into());
    }
    Ok(comps)
}

/// Resolves parsed path components over an upstream node table (in
/// decoded-body coordinates): returns the flat per-component records
/// `(is_index, idx, key bytes)` and the final value-node index.
pub(crate) fn resolve_path<'a>(
    nodes: &[JsonNode],
    dbody: &[u8],
    comps: &[Comp<'a>],
) -> Result<(Vec<(u32, u32, &'a [u8])>, usize), String> {
    let mut resolved: Vec<(u32, u32, &[u8])> = Vec::new();
    let mut cur = 0usize;
    for comp in comps {
        let nd = &nodes[cur];
        match comp {
            Comp::Key(want) => {
                if nd.kind != JsonKind::Object {
                    return Err(format!("path runs through a non-object at node {cur}"));
                }
                let limit = cur + nd.size as usize;
                let mut child = cur + 1;
                let mut hit = None;
                while child < limit {
                    let key: &JsonNode = &nodes[child];
                    if &dbody[key.start as usize..key.end as usize] == *want {
                        hit = Some(child);
                    }
                    child = child + 1 + nodes[child + 1].size as usize;
                }
                let key_idx = hit.ok_or_else(|| {
                    format!("path component {:?} not found", String::from_utf8_lossy(want))
                })?;
                resolved.push((0, key_idx as u32, want));
                cur = key_idx + 1;
            }
            Comp::Index(want) => {
                if nd.kind != JsonKind::Array {
                    return Err(format!("path indexes into a non-array at node {cur}"));
                }
                let limit = cur + nd.size as usize;
                let mut child = cur + 1;
                for _ in 0..*want {
                    if child >= limit {
                        return Err(format!("array index {want} out of range"));
                    }
                    child += nodes[child].size as usize;
                }
                if child >= limit {
                    return Err(format!("array index {want} out of range"));
                }
                resolved.push((1, *want as u32, b""));
                cur = child;
            }
        }
    }
    Ok((resolved, cur))
}

/// Kind tag in the flat encoding (matches upstream's `#[repr(u8)]` order).
pub(crate) fn kind_code(k: JsonKind) -> u32 {
    match k {
        JsonKind::Null => 0,
        JsonKind::Bool => 1,
        JsonKind::Number => 2,
        JsonKind::String => 3,
        JsonKind::Key => 4,
        JsonKind::Object => 5,
        JsonKind::Array => 6,
    }
}

/// [`encode_claim`] for the original JSON-path claim.
pub fn encode(
    sent: &[u8],
    recv: &[u8],
    table: &SpanTable,
    path: &str,
    expect: Option<&str>,
) -> Result<Encoded, String> {
    encode_claim(sent, recv, table, &Claim::Json { path }, expect)
}

/// Encodes the claim "`sent`/`recv` is a well-formed exchange with this
/// method, target, `Host`, and status, plus `claim`" as the guest's flat
/// public table. With `expect = None` the claimed value's bytes are
/// disclosed; with `Some(v)` the guest instead asserts they equal `v` and
/// reveals only the 0/1 flag (a mismatched `v` encodes fine — the proof
/// then legitimately fails).
pub fn encode_claim<'a>(
    sent: &'a [u8],
    recv: &'a [u8],
    table: &'a SpanTable,
    claim: &Claim<'a>,
    expect: Option<&str>,
) -> Result<Encoded, String> {
    if sent.len() > SENT_CAP {
        return Err(format!("request larger than {SENT_CAP} bytes"));
    }
    if recv.len() > RECV_CAP {
        return Err(format!("response larger than {RECV_CAP} bytes"));
    }
    // The real upstream validator first: everything below may then trust
    // the spans (the flat verifier still re-checks all of it in the VM).
    validate(sent, recv, table).map_err(|e| format!("transcript-verify rejected: {e}"))?;

    let req = &table.request;
    let resp = &table.response;
    if let Some(req_body) = &req.body {
        // Covered as opaque private bytes; only the framing must be in
        // scope (the flat verifier re-derives it from the CL header).
        if req_body.framing != Framing::ContentLength {
            return Err("only Content-Length framing is in scope for this demo".into());
        }
    }
    let body = resp.body.as_ref().ok_or("response has no body")?;
    if body.framing != Framing::ContentLength {
        return Err("only Content-Length framing is in scope for this demo".into());
    }
    let head_end = resp.head_end as usize;

    let method = &sent[req.method.as_range()];
    if method == b"HEAD" {
        return Err("HEAD requests are out of scope for this demo".into());
    }
    let target = &sent[req.target.as_range()];

    // First match is the only match: upstream validation already rejected
    // duplicate `Host` / `Content-Length` headers.
    let find_header = |headers: &[HeaderSpan], buf: &[u8], name: &[u8]| -> Option<usize> {
        headers
            .iter()
            .position(|h| buf[h.name.as_range()].eq_ignore_ascii_case(name))
    };
    let host_idx =
        find_header(&req.headers, sent, b"host").ok_or("request has no Host header")?;
    let host_value = &sent[req.headers[host_idx].value.as_range()];
    let cl_idx = find_header(&resp.headers, recv, b"content-length")
        .ok_or("response has no Content-Length header")?;
    // Meaningful only with a request body; pinned to 0 otherwise.
    let req_cl_idx = if req.body.is_some() {
        find_header(&req.headers, sent, b"content-length")
            .ok_or("request body without a Content-Length header")?
    } else {
        0
    };

    if req.headers.len() > MAX_HEADERS || resp.headers.len() > MAX_HEADERS {
        return Err(format!("more than {MAX_HEADERS} header lines"));
    }

    let status = (recv[9] - b'0') as u16 * 100 + (recv[10] - b'0') as u16 * 10
        + (recv[11] - b'0') as u16;

    // === resolve the claim ===
    let mut json_claim: Option<(&'a [JsonNode], Vec<(u32, u32, &'a [u8])>, usize)> = None;
    let mut hdr_claim: Option<(usize, Vec<u8>)> = None; // (index, lowercase name)
    let (target_code, value_start, value_len, value_buf): (u32, usize, usize, &[u8]) = match claim
    {
        Claim::Json { path } => {
            let json = body
                .json
                .as_ref()
                .ok_or("the response body is not claimed as JSON")?;
            // Demo cut: the verifier checks JSON strings as ASCII, not
            // UTF-8.
            if recv[head_end..].iter().any(|&b| b >= 0x80) {
                return Err("non-ASCII JSON bodies are out of scope for this demo".into());
            }
            let nodes: &'a [JsonNode] = &json.nodes;
            let comps = parse_path(path)?;
            let (resolved, cur) = resolve_path(nodes, &recv[head_end..], &comps)?;
            let value = &nodes[cur];
            match value.kind {
                JsonKind::Null | JsonKind::Bool | JsonKind::Number | JsonKind::String => {}
                _ => {
                    return Err(
                        "the path must point to a scalar (string, number, bool, null)".into()
                    );
                }
            }
            let value_len = (value.end - value.start) as usize;
            if value_len > OUT_CAP {
                return Err(format!("the value is larger than {OUT_CAP} bytes"));
            }
            // Decoded-body → `recv` source coordinates (identical layout
            // under Content-Length framing, shifted by the head).
            let value_start = head_end + value.start as usize;
            json_claim = Some((nodes, resolved, cur));
            (TARGET_JSON as u32, value_start, value_len, recv)
        }
        Claim::RequestHeader { index } | Claim::ResponseHeader { index } => {
            let in_sent = matches!(claim, Claim::RequestHeader { .. });
            let (headers, buf): (&[HeaderSpan], &'a [u8]) = if in_sent {
                (&req.headers, sent)
            } else {
                (&resp.headers, recv)
            };
            let h = headers
                .get(*index)
                .ok_or("claimed header index out of range")?;
            let value_start = h.value.start as usize;
            let value_len = (h.value.end - h.value.start) as usize;
            if value_len > OUT_CAP {
                return Err(format!("the value is larger than {OUT_CAP} bytes"));
            }
            // The public claim names the header in lowercase; the verifier
            // matches the private bytes case-insensitively.
            hdr_claim = Some((*index, buf[h.name.as_range()].to_ascii_lowercase()));
            let code = if in_sent { TARGET_REQ_HEADER } else { TARGET_RESP_HEADER };
            (code as u32, value_start, value_len, buf)
        }
    };

    // === assemble the words ===
    let req_hdrs_off = HEADER_WORDS;
    let resp_hdrs_off = req_hdrs_off + 4 * req.headers.len();
    let sections_end = resp_hdrs_off + 4 * resp.headers.len();
    // The node/path sections exist only for a JSON claim; their offset
    // words are pinned to 0 otherwise (the verifier requires it).
    let (nodes_off, path_off, str_off) = match &json_claim {
        Some((nodes, resolved, _)) => {
            let nodes_off = sections_end;
            let path_off = nodes_off + 4 * nodes.len();
            (nodes_off, path_off, path_off + 4 * resolved.len())
        }
        None => (0, 0, sections_end),
    };

    // Packed string region: method ++ target ++ host value ++ the claim's
    // strings (key components / header name) ++ the expected value.
    let mut strs: Vec<u8> = Vec::new();
    let mut put_str = |bytes: &[u8]| -> (u32, u32) {
        let off = strs.len() as u32;
        strs.extend_from_slice(bytes);
        (off, bytes.len() as u32)
    };
    let (method_soff, method_slen) = put_str(method);
    let (target_soff, target_slen) = put_str(target);
    let (host_soff, host_slen) = put_str(host_value);
    let comp_strs: Vec<(u32, u32)> = match &json_claim {
        Some((_, resolved, _)) => resolved.iter().map(|(_, _, k)| put_str(k)).collect(),
        None => Vec::new(),
    };
    let (name_soff, name_slen) = match &hdr_claim {
        Some((_, name)) => put_str(name),
        None => (0, 0),
    };
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
    w[W_SENT_LEN] = sent.len() as u32;
    w[W_RECV_LEN] = recv.len() as u32;
    w[W_METHOD_END] = req.method.end;
    w[W_TARGET_END] = req.target.end;
    w[W_REQ_HEAD_END] = req.head_end;
    w[W_REQ_N_HDRS] = req.headers.len() as u32;
    w[W_REQ_HDRS_OFF] = req_hdrs_off as u32;
    w[W_REQ_HOST_IDX] = host_idx as u32;
    w[W_REASON_START] = resp.reason.start;
    w[W_REASON_END] = resp.reason.end;
    w[W_RESP_HEAD_END] = resp.head_end;
    w[W_RESP_N_HDRS] = resp.headers.len() as u32;
    w[W_RESP_HDRS_OFF] = resp_hdrs_off as u32;
    w[W_RESP_CL_IDX] = cl_idx as u32;
    w[W_STATUS] = status as u32;
    if let Some((nodes, resolved, cur)) = &json_claim {
        w[W_N_NODES] = nodes.len() as u32;
        w[W_NODES_OFF] = nodes_off as u32;
        w[W_N_PATH] = resolved.len() as u32;
        // Pinned to 0 when there are no components (the verifier requires
        // it, so the word has no slack when it is meaningless).
        w[W_PATH_OFF] = if resolved.is_empty() { 0 } else { path_off as u32 };
        w[W_VALUE_NODE] = *cur as u32;
    }
    w[W_STR_OFF] = str_off as u32;
    w[W_STR_BYTES] = strs.len() as u32;
    w[W_METHOD_SOFF] = method_soff;
    w[W_METHOD_SLEN] = method_slen;
    w[W_TARGET_SOFF] = target_soff;
    w[W_TARGET_SLEN] = target_slen;
    w[W_HOST_SOFF] = host_soff;
    w[W_HOST_SLEN] = host_slen;
    w[W_REQ_CL_IDX] = req_cl_idx as u32;
    w[W_CLAIM_MODE] = if expect.is_some() {
        MODE_ASSERT as u32
    } else {
        MODE_DISCLOSE as u32
    };
    w[W_EXPECT_SOFF] = expect_soff;
    w[W_EXPECT_SLEN] = expect_slen;
    w[W_CLAIM_TARGET] = target_code;
    if let Some((index, _)) = &hdr_claim {
        w[W_HDR_IDX] = *index as u32;
        w[W_HDR_NAME_SOFF] = name_soff;
        w[W_HDR_NAME_SLEN] = name_slen;
    }

    let put_hdrs = |w: &mut [u32], off: usize, hs: &[HeaderSpan]| {
        for (i, h) in hs.iter().enumerate() {
            w[off + 4 * i] = h.name.start;
            w[off + 4 * i + 1] = h.name.end;
            w[off + 4 * i + 2] = h.value.start;
            w[off + 4 * i + 3] = h.value.end;
        }
    };
    put_hdrs(&mut w, req_hdrs_off, &req.headers);
    put_hdrs(&mut w, resp_hdrs_off, &resp.headers);

    if let Some((nodes, resolved, _)) = &json_claim {
        for (i, n) in nodes.iter().enumerate() {
            // Decoded-body → `recv` source coordinates (identical layout
            // under Content-Length framing, shifted by the head).
            w[nodes_off + 4 * i] = kind_code(n.kind);
            w[nodes_off + 4 * i + 1] = n.start + head_end as u32;
            w[nodes_off + 4 * i + 2] = n.end + head_end as u32;
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
    }

    for (j, &b) in strs.iter().enumerate() {
        w[str_off + j / 4] |= (b as u32) << ((j % 4) * 8);
    }

    // Self-check: the flat verifier must accept its own encoding — except
    // that an assert against a non-matching expected value must (only)
    // fail the byte checks.
    let value_bytes = &value_buf[value_start..value_start + value_len];
    let should_hold = expect.is_none_or(|e| e.as_bytes() == value_bytes);
    let (want_start, want_len) = if expect.is_some() {
        (0, 0)
    } else {
        (value_start, value_len)
    };
    let want_in_sent = expect.is_none() && matches!(claim, Claim::RequestHeader { .. });
    match crate::verify(sent, recv, &w) {
        Ok(d)
            if d.ok == should_hold as i32
                && d.value_start == want_start
                && d.value_len == want_len
                && d.value_in_sent == want_in_sent => {}
        Ok(_) => return Err("internal: flat encoding failed its byte checks".into()),
        Err(e) => return Err(format!("internal: flat encoding rejected: {e}")),
    }

    Ok(Encoded {
        words: w,
        status,
        value_start,
        value_len,
    })
}
