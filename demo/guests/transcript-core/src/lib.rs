//! Branch-free verification of an HTTP/JSON transcript against a public
//! span table, for the Speakup zk-vm.
//!
//! This is the demo's port of the `transcript-verify` advice pattern to a
//! zk-vm that cannot branch on private data. The roles are inverted:
//! upstream, the (private) bytes drive a cursor and the table is
//! equality-checked against it; here the (public) table drives the cursor —
//! all control flow, loop bounds, and positions are concrete — and the
//! private bytes are equality-checked at every position the table claims.
//! The byte-level language accepted is the same, so for fixed bytes at most
//! one table validates (canonicality), and every private byte of both
//! buffers is covered by exactly one check (no smuggling).
//!
//! The flat table is a `u32` word array (see the `W_*` layout constants)
//! carrying the spans of `transcript_verify::SpanTable` plus the public
//! claims of the demo statement: the request method/target, the request
//! `Host` value, the response status code, and a JSON path (key bytes +
//! node indices) naming one scalar value to disclose.
//!
//! [`verify`] returns concrete `Err` for a malformed *table* (public data —
//! both parties reject identically before any private work) and a symbolic
//! 0/1 flag for the *byte* checks, which the guest reveals along with the
//! masked value bytes.
//!
//! Demo-scope cuts relative to upstream `transcript-verify` (each is
//! strictly *stricter* — valid transcripts may be rejected, invalid ones are
//! never accepted):
//!
//! - `Content-Length` framing only (no chunked, no close-delimited);
//!   request bodies are covered as OPAQUE private bytes (framing verified,
//!   contents unconstrained and unclaimed);
//! - JSON string/key content must be ASCII (upstream allows UTF-8);
//! - duplicate object keys are rejected by *raw* byte comparison (upstream
//!   compares escape-decoded keys, so an escaped alias of a key is treated
//!   as a duplicate there but as distinct here — the path binding below is
//!   unaffected, because it also compares raw bytes);
//! - lone-surrogate pairing in `\u` escapes is not analyzed (same as
//!   upstream's scanner);
//! - numbers at most [`MAX_NUMBER_LEN`] bytes, `Content-Length` values at
//!   most 9 digits, and the size caps below.

use core::hint::black_box;

pub mod encode;
pub mod json;

/// Flat-table format version (independent of upstream's `FORMAT_VERSION`).
/// v2: claim targets — the claim may name one header line instead of a
/// JSON body value, in which case the response body is covered opaquely.
pub const VERSION: u32 = 2;

/// Maximum private request buffer size.
pub const SENT_CAP: usize = 1024;
/// Maximum private response buffer size.
pub const RECV_CAP: usize = 4096;
/// Maximum flat-table size in words.
pub const TABLE_CAP_WORDS: usize = 2048;
/// Maximum disclosed-value size (also the guest's output buffer size).
pub const OUT_CAP: usize = 256;
/// Maximum header lines per message.
pub const MAX_HEADERS: usize = 48;
/// Maximum JSON nesting depth.
pub const MAX_DEPTH: usize = 16;
/// Maximum members per JSON object.
pub const MAX_KEYS: usize = 64;
/// Maximum JSON path components.
pub const MAX_PATH: usize = 8;
/// Maximum bytes of one number lexeme.
pub const MAX_NUMBER_LEN: usize = 32;

// === flat-table header word indices ===

pub const W_VERSION: usize = 0;
pub const W_SENT_LEN: usize = 1;
pub const W_RECV_LEN: usize = 2;
pub const W_METHOD_END: usize = 3;
pub const W_TARGET_END: usize = 4;
pub const W_REQ_HEAD_END: usize = 5;
pub const W_REQ_N_HDRS: usize = 6;
pub const W_REQ_HDRS_OFF: usize = 7;
pub const W_REQ_HOST_IDX: usize = 8;
pub const W_REASON_START: usize = 9;
pub const W_REASON_END: usize = 10;
pub const W_RESP_HEAD_END: usize = 11;
pub const W_RESP_N_HDRS: usize = 12;
pub const W_RESP_HDRS_OFF: usize = 13;
pub const W_RESP_CL_IDX: usize = 14;
pub const W_STATUS: usize = 15;
pub const W_N_NODES: usize = 16;
pub const W_NODES_OFF: usize = 17;
pub const W_N_PATH: usize = 18;
pub const W_PATH_OFF: usize = 19;
pub const W_VALUE_NODE: usize = 20;
pub const W_STR_OFF: usize = 21;
pub const W_STR_BYTES: usize = 22;
pub const W_METHOD_SOFF: usize = 23;
pub const W_METHOD_SLEN: usize = 24;
pub const W_TARGET_SOFF: usize = 25;
pub const W_TARGET_SLEN: usize = 26;
pub const W_HOST_SOFF: usize = 27;
pub const W_HOST_SLEN: usize = 28;
/// Index of the request's `Content-Length` header record; must be 0 when
/// the request has no body (`req_head_end == sent_len`).
pub const W_REQ_CL_IDX: usize = 29;
/// 0 = disclose the value's bytes; 1 = assert they equal the public
/// expected string (nothing revealed but the 0/1 flag).
pub const W_CLAIM_MODE: usize = 30;
/// The expected-value string (assert mode; must be 0/0 in disclose mode).
pub const W_EXPECT_SOFF: usize = 31;
pub const W_EXPECT_SLEN: usize = 32;
/// What the claim targets: a JSON body value ([`TARGET_JSON`]) or one
/// header line ([`TARGET_REQ_HEADER`]/[`TARGET_RESP_HEADER`]). With a
/// header target the JSON section must be empty (the response body is
/// covered as opaque private bytes, like request bodies).
pub const W_CLAIM_TARGET: usize = 33;
/// Header-claim words (must all be 0 for a JSON claim): the claimed
/// line's index on its side, and the public header name (lowercase).
pub const W_HDR_IDX: usize = 34;
pub const W_HDR_NAME_SOFF: usize = 35;
pub const W_HDR_NAME_SLEN: usize = 36;
/// Number of fixed header words; the variable sections follow.
pub const HEADER_WORDS: usize = 37;

pub const MODE_DISCLOSE: usize = 0;
pub const MODE_ASSERT: usize = 1;

pub const TARGET_JSON: usize = 0;
pub const TARGET_REQ_HEADER: usize = 1;
pub const TARGET_RESP_HEADER: usize = 2;

// JSON node kinds — numbering matches `transcript_verify::JsonKind`.
pub const KIND_NULL: usize = 0;
pub const KIND_BOOL: usize = 1;
pub const KIND_NUMBER: usize = 2;
pub const KIND_STRING: usize = 3;
pub const KIND_KEY: usize = 4;
pub const KIND_OBJECT: usize = 5;
pub const KIND_ARRAY: usize = 6;

/// The verification outcome: a symbolic 0/1 validity flag plus the
/// (public) span of the value to disclose.
pub struct Disclosure {
    /// 1 iff every private byte check passed. Symbolic in the VM.
    pub ok: i32,
    /// First byte of the disclosed value's content, in `sent` coordinates
    /// when `value_in_sent` (a request-header claim), else in
    /// `recv`/document coordinates.
    pub value_start: usize,
    /// Content length in bytes (`<=` [`OUT_CAP`]).
    pub value_len: usize,
    /// Whether the disclosed bytes live in the request buffer. Concrete —
    /// derived from the public claim target, identical on both parties.
    pub value_in_sent: bool,
}

// === pinned-flag helpers ===
//
// Every flag derived from private data is pinned with `black_box` AT
// CREATION: LLVM otherwise proves flags are 0/1 and lowers the mask idioms
// below into `select`, which the zk-vm rejects (see CLAUDE.md, "the one
// rule"). Pinning concrete flags too is harmless — black_box is a compiler
// barrier, not a VM op, and concrete data stays concrete.

#[inline(always)]
fn bb(x: i32) -> i32 {
    black_box(x)
}

/// A pinned 0/1 flag from a boolean.
#[inline(always)]
fn f(c: bool) -> i32 {
    bb(c as i32)
}

#[inline(always)]
fn eq(b: i32, c: u8) -> i32 {
    f(b == c as i32)
}

#[inline(always)]
fn in_range(b: i32, lo: u8, hi: u8) -> i32 {
    bb(f(b >= lo as i32) & f(b <= hi as i32))
}

/// OWS: SP or HTAB.
#[inline(always)]
fn is_ows(b: i32) -> i32 {
    bb(eq(b, b' ') | eq(b, b'\t'))
}

/// JSON whitespace (RFC 8259 `ws`): SP, HTAB, LF, CR.
#[inline(always)]
fn is_jws(b: i32) -> i32 {
    bb(eq(b, b' ') | eq(b, b'\t') | eq(b, b'\n') | eq(b, b'\r'))
}

#[inline(always)]
fn is_digit(b: i32) -> i32 {
    in_range(b, b'0', b'9')
}

#[inline(always)]
fn is_hexdigit(b: i32) -> i32 {
    bb(is_digit(b) | in_range(b, b'a', b'f') | in_range(b, b'A', b'F'))
}

/// Header-value (and reason) byte: {0x21..=0x7E, 0x80..=0xFF, SP, HTAB}.
#[inline(always)]
fn is_value_byte(b: i32) -> i32 {
    bb(eq(b, b'\t') | bb(f(b >= 0x20) & f(b != 0x7F)))
}

/// RFC 9110 `tchar`.
#[inline(always)]
fn is_tchar(b: i32) -> i32 {
    let alnum = bb(is_digit(b) | in_range(b, b'a', b'z') | in_range(b, b'A', b'Z'));
    let sym = bb(eq(b, b'!')
        | eq(b, b'#')
        | eq(b, b'$')
        | eq(b, b'%')
        | eq(b, b'&')
        | eq(b, b'\'')
        | eq(b, b'*')
        | eq(b, b'+')
        | eq(b, b'-')
        | eq(b, b'.')
        | eq(b, b'^')
        | eq(b, b'_')
        | eq(b, b'`')
        | eq(b, b'|')
        | eq(b, b'~'));
    bb(alnum | sym)
}

#[inline(always)]
fn byte(buf: &[u8], i: usize) -> i32 {
    buf[i] as i32
}

// === private byte-check primitives (concrete positions, private bytes) ===

/// `buf[at..at+lit.len()] == lit`, accumulated into `bad`.
fn check_lit(buf: &[u8], at: usize, lit: &[u8], bad: &mut i32) {
    for (i, &c) in lit.iter().enumerate() {
        *bad = bb(*bad | f(byte(buf, at + i) != c as i32));
    }
}

/// `buf[span] == expected` (a public byte string), accumulated into `bad`.
fn check_eq_pub(buf: &[u8], start: usize, expected: &[u8], bad: &mut i32) {
    check_lit(buf, start, expected, bad);
}

/// Case-insensitive match of `buf[ns..ne]` against the lowercase ASCII
/// `lit`; returns a pinned 0/1 flag (concrete 0 if the lengths differ).
fn name_eq_ci(buf: &[u8], ns: usize, ne: usize, lit: &[u8]) -> i32 {
    if ne - ns != lit.len() {
        return 0;
    }
    let mut diff = 0i32;
    for (i, &c) in lit.iter().enumerate() {
        let b = byte(buf, ns + i);
        let m = if c.is_ascii_lowercase() {
            f((b | 0x20) == c as i32)
        } else {
            f(b == c as i32)
        };
        diff = bb(diff | f(m == 0));
    }
    f(diff == 0)
}

/// Whether `buf[a..a+len] == buf[b..b+len]` (both private); pinned 0/1.
fn bytes_eq_priv(buf: &[u8], a: usize, b: usize, len: usize) -> i32 {
    let mut diff = 0i32;
    for i in 0..len {
        diff = bb(diff | f(byte(buf, a + i) != byte(buf, b + i)));
    }
    f(diff == 0)
}

/// A whitespace gap `[from, to)`: every byte JSON whitespace, plus — when
/// `sep` is given — exactly one occurrence of the separator (the language
/// `ws* sep ws*`, same as upstream's skip/expect/skip).
fn check_gap(buf: &[u8], from: usize, to: usize, sep: Option<u8>, bad: &mut i32) {
    match sep {
        None => {
            for i in from..to {
                *bad = bb(*bad | f(is_jws(byte(buf, i)) == 0));
            }
        }
        Some(s) => {
            let mut count = 0i32;
            for i in from..to {
                let b = byte(buf, i);
                let is_sep = eq(b, s);
                *bad = bb(*bad | f(bb(is_jws(b) | is_sep) == 0));
                count = bb(count.wrapping_add(is_sep));
            }
            *bad = bb(*bad | f(count != 1));
        }
    }
}

/// One RFC 8259 string's content bytes `[start, end)` (quotes excluded):
/// escape grammar, no unescaped quote, no raw control bytes, ASCII only
/// (demo cut), and no escape left open at the end.
fn check_string_content(buf: &[u8], start: usize, end: usize, bad: &mut i32) {
    let mut esc = 0i32; // previous byte opened an escape
    let mut hex = 0i32; // hex digits still owed to a \u escape
    for i in start..end {
        let b = byte(buf, i);
        let was_esc = esc;
        let was_hex = f(hex > 0);
        // Inside \uXXXX: the byte must be a hex digit.
        *bad = bb(*bad | bb(was_hex & f(is_hexdigit(b) == 0)));
        hex = bb(hex.wrapping_sub(was_hex));
        // Directly after a backslash: one of the simple escapes or `u`.
        let simple = bb(eq(b, b'"')
            | eq(b, b'\\')
            | eq(b, b'/')
            | eq(b, b'b')
            | eq(b, b'f')
            | eq(b, b'n')
            | eq(b, b'r')
            | eq(b, b't'));
        let is_u = eq(b, b'u');
        *bad = bb(*bad | bb(was_esc & f(bb(simple | is_u) == 0)));
        hex = bb(hex.wrapping_add(bb(was_esc & is_u).wrapping_mul(4)));
        // A plain content byte: no unescaped quote; a backslash opens an
        // escape.
        let norm = bb(f(was_esc == 0) & f(was_hex == 0));
        *bad = bb(*bad | bb(norm & eq(b, b'"')));
        esc = bb(norm & eq(b, b'\\'));
        // Raw control bytes are illegal everywhere; >= 0x80 is the demo's
        // ASCII-only cut.
        *bad = bb(*bad | f(b < 0x20) | f(b >= 0x80));
    }
    // An escape or \u run must not extend past the closing quote.
    *bad = bb(*bad | esc | f(hex != 0));
}

/// One RFC 8259 number lexeme `[start, end)`:
/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, the whole span
/// consumed. Maximality is enforced by the caller's gap tiling (the byte
/// after the lexeme must be whitespace or a separator), exactly as
/// upstream's rule F7.
fn check_number(buf: &[u8], start: usize, end: usize, bad: &mut i32) {
    // One-hot DFA states.
    let mut s_start = 1i32;
    let mut s_minus = 0i32;
    let mut s_zero = 0i32;
    let mut s_int = 0i32;
    let mut s_dot = 0i32;
    let mut s_frac = 0i32;
    let mut s_e = 0i32;
    let mut s_esign = 0i32;
    let mut s_exp = 0i32;
    for i in start..end {
        let b = byte(buf, i);
        let d = is_digit(b);
        let d19 = in_range(b, b'1', b'9');
        let z = eq(b, b'0');
        let dot = eq(b, b'.');
        let ee = bb(eq(b, b'e') | eq(b, b'E'));
        let pm = bb(eq(b, b'+') | eq(b, b'-'));
        let minus = eq(b, b'-');

        let n_minus = bb(s_start & minus);
        let n_zero = bb(bb(s_start | s_minus) & z);
        let n_int = bb(bb(bb(s_start | s_minus) & d19) | bb(s_int & d));
        let n_dot = bb(bb(s_zero | s_int) & dot);
        let n_frac = bb(bb(s_dot | s_frac) & d);
        let n_e = bb(bb(s_zero | s_int | s_frac) & ee);
        let n_esign = bb(s_e & pm);
        let n_exp = bb(bb(s_e | s_esign | s_exp) & d);

        s_start = 0;
        s_minus = bb(n_minus);
        s_zero = bb(n_zero);
        s_int = bb(n_int);
        s_dot = bb(n_dot);
        s_frac = bb(n_frac);
        s_e = bb(n_e);
        s_esign = bb(n_esign);
        s_exp = bb(n_exp);
    }
    let accept = bb(s_zero | s_int | s_frac | s_exp);
    *bad = bb(*bad | f(accept == 0));
}

// === concrete table access ===

fn gw(table: &[u32], i: usize) -> Result<usize, &'static str> {
    let w = *table.get(i).ok_or("table too short")?;
    // Same coordinate invariant as upstream: keeps every sum of two table
    // values overflow-free, including on 32-bit targets.
    if w > (1 << 30) {
        return Err("table value too large");
    }
    Ok(w as usize)
}

/// One byte of the packed public string region (raw words — packed bytes
/// routinely exceed the coordinate bound `gw` enforces).
fn str_byte(table: &[u32], str_off: usize, j: usize) -> Result<u8, &'static str> {
    let w = *table.get(str_off + j / 4).ok_or("table too short")?;
    Ok(((w >> ((j % 4) * 8)) & 0xFF) as u8)
}

fn str_slice(
    table: &[u32],
    str_off: usize,
    str_bytes: usize,
    soff: usize,
    slen: usize,
    out: &mut [u8],
) -> Result<usize, &'static str> {
    if soff + slen > str_bytes || slen > out.len() {
        return Err("string region overflow");
    }
    for j in 0..slen {
        out[j] = str_byte(table, str_off, soff + j)?;
    }
    Ok(slen)
}

/// The (public) disclosed-value content span recorded in a flat table:
/// `(start, len)` in the claimed buffer's coordinates — `(0, 0)` in assert
/// mode, where nothing is revealed but the flag. Both hosts use it to size
/// the readback of the guest's revealed output buffer; [`verify`]
/// re-derives and fully checks the same span in the VM.
pub fn disclosure_span(table: &[u32]) -> Result<(usize, usize), &'static str> {
    if gw(table, W_CLAIM_MODE)? == MODE_ASSERT {
        return Ok((0, 0));
    }
    match gw(table, W_CLAIM_TARGET)? {
        TARGET_JSON => {
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
        t @ (TARGET_REQ_HEADER | TARGET_RESP_HEADER) => {
            let (off_w, n_w) = if t == TARGET_REQ_HEADER {
                (W_REQ_HDRS_OFF, W_REQ_N_HDRS)
            } else {
                (W_RESP_HDRS_OFF, W_RESP_N_HDRS)
            };
            let idx = gw(table, W_HDR_IDX)?;
            if idx >= gw(table, n_w)? {
                return Err("claimed header index out of range");
            }
            let off = gw(table, off_w)?;
            let vs = gw(table, off + 4 * idx + 2)?;
            let ve = gw(table, off + 4 * idx + 3)?;
            if ve < vs || ve - vs > OUT_CAP {
                return Err("value span invalid");
            }
            Ok((vs, ve - vs))
        }
        _ => Err("unknown claim target"),
    }
}

#[derive(Clone, Copy, Default)]
struct Hdr {
    ns: usize,
    ne: usize,
    vs: usize,
    ve: usize,
    /// Position of this line's CR (concrete, derived from the tiling).
    cr: usize,
}

#[derive(Clone, Copy, Default)]
struct Node {
    kind: usize,
    start: usize,
    end: usize,
    size: usize,
}

fn get_node(table: &[u32], nodes_off: usize, i: usize) -> Result<Node, &'static str> {
    let base = nodes_off + 4 * i;
    Ok(Node {
        kind: gw(table, base)?,
        start: gw(table, base + 1)?,
        end: gw(table, base + 2)?,
        size: gw(table, base + 3)?,
    })
}

/// Reads and concretely shape-checks one message's header records: spans in
/// bijection with the lines tiling `[line0, head_end - 2)`, the blank-line
/// CRLF closing the head, value spans ordered and OWS-canonical-compatible,
/// empty values pinned at the CR.
fn shape_headers(
    table: &[u32],
    off: usize,
    n: usize,
    line0: usize,
    head_end: usize,
    buf_len: usize,
) -> Result<[Hdr; MAX_HEADERS], &'static str> {
    if n > MAX_HEADERS {
        return Err("too many headers");
    }
    if head_end > buf_len || head_end < line0 + 2 {
        return Err("bad head extent");
    }
    let mut hs = [Hdr::default(); MAX_HEADERS];
    let mut cursor = line0;
    for i in 0..n {
        let base = off + 4 * i;
        let h = Hdr {
            ns: gw(table, base)?,
            ne: gw(table, base + 1)?,
            vs: gw(table, base + 2)?,
            ve: gw(table, base + 3)?,
            cr: 0,
        };
        let line_end = if i + 1 < n {
            gw(table, off + 4 * (i + 1))?
        } else {
            head_end - 2
        };
        if line_end < cursor + 2 {
            return Err("header line out of order");
        }
        let cr = line_end - 2;
        // name: nonempty tchar run from the line start; colon at ne < cr.
        if h.ns != cursor || h.ne <= h.ns || h.ne + 1 > cr {
            return Err("header name span out of place");
        }
        if h.vs == h.ve {
            // Empty value: pinned at the CR.
            if h.vs != cr {
                return Err("empty header value not pinned at CR");
            }
        } else if h.vs < h.ne + 1 || h.ve <= h.vs || h.ve > cr {
            return Err("header value span out of place");
        }
        hs[i] = Hdr { cr, ..h };
        cursor = line_end;
    }
    if cursor != head_end - 2 {
        return Err("header lines do not tile the head");
    }
    Ok(hs)
}

/// The private byte checks for one message's header lines (shape already
/// validated): tchar names, the colon, OWS runs, value charset and
/// canonical trim, and every line's CRLF — total coverage of
/// `[line0, head_end)` together with the blank line.
fn check_headers(buf: &[u8], hs: &[Hdr], n: usize, head_end: usize, bad: &mut i32) {
    for h in hs.iter().take(n) {
        for i in h.ns..h.ne {
            *bad = bb(*bad | f(is_tchar(byte(buf, i)) == 0));
        }
        *bad = bb(*bad | f(eq(byte(buf, h.ne), b':') == 0));
        let ows_end = if h.vs == h.ve { h.cr } else { h.vs };
        for i in h.ne + 1..ows_end {
            *bad = bb(*bad | f(is_ows(byte(buf, i)) == 0));
        }
        if h.vs < h.ve {
            // Canonical trim: first and last value bytes are non-OWS.
            *bad = bb(*bad | is_ows(byte(buf, h.vs)));
            *bad = bb(*bad | is_ows(byte(buf, h.ve - 1)));
            for i in h.vs..h.ve {
                *bad = bb(*bad | f(is_value_byte(byte(buf, i)) == 0));
            }
            for i in h.ve..h.cr {
                *bad = bb(*bad | f(is_ows(byte(buf, i)) == 0));
            }
        }
        check_lit(buf, h.cr, b"\r\n", bad);
    }
    // The blank line terminating the head.
    check_lit(buf, head_end - 2, b"\r\n", bad);
}

/// The digits of a claimed `Content-Length` header value must equal the
/// body extent: concrete length cap (9 digits keep the fold in `i32`),
/// private per-byte digit gates and the fold itself.
fn check_cl_digits(buf: &[u8], h: &Hdr, body_len: usize, bad: &mut i32) -> Result<(), &'static str> {
    let cl_len = h.ve - h.vs;
    if cl_len == 0 || cl_len > 9 {
        return Err("content-length value length out of range");
    }
    let mut acc = 0i32;
    for i in h.vs..h.ve {
        let b = byte(buf, i);
        *bad = bb(*bad | f(is_digit(b) == 0));
        acc = bb(acc.wrapping_mul(10).wrapping_add(b.wrapping_sub(b'0' as i32)));
    }
    *bad = bb(*bad | f(acc != body_len as i32));
    Ok(())
}

/// First byte position of a JSON *value* node (strings exclude their
/// quotes, so their first byte is the opening quote at `start - 1`).
fn first_byte_pos(nd: &Node) -> Result<usize, &'static str> {
    match nd.kind {
        KIND_STRING => nd.start.checked_sub(1).ok_or("string span at 0"),
        KIND_KEY => Err("key node at value position"),
        KIND_NULL | KIND_BOOL | KIND_NUMBER | KIND_OBJECT | KIND_ARRAY => Ok(nd.start),
        _ => Err("unknown node kind"),
    }
}

/// Verifies the private `sent`/`recv` bytes against the public flat
/// `table`. Concrete `Err` = malformed table (both parties reject it
/// identically, outside the private domain); otherwise the returned
/// [`Disclosure::ok`] is the symbolic conjunction of every byte check.
pub fn verify(sent: &[u8], recv: &[u8], table: &[u32]) -> Result<Disclosure, &'static str> {
    let mut bad = 0i32;

    // === table header, lengths, claims (all concrete) ===
    if gw(table, W_VERSION)? != VERSION as usize {
        return Err("unsupported table version");
    }
    if gw(table, W_SENT_LEN)? != sent.len() || gw(table, W_RECV_LEN)? != recv.len() {
        return Err("table/buffer length mismatch");
    }
    if sent.len() > SENT_CAP || recv.len() > RECV_CAP || table.len() > TABLE_CAP_WORDS {
        return Err("input too large");
    }
    let str_off = gw(table, W_STR_OFF)?;
    let str_bytes = gw(table, W_STR_BYTES)?;
    if str_off + str_bytes.div_ceil(4) > table.len() {
        return Err("string region out of bounds");
    }

    let mut method = [0u8; 32];
    let method_len = str_slice(
        table,
        str_off,
        str_bytes,
        gw(table, W_METHOD_SOFF)?,
        gw(table, W_METHOD_SLEN)?,
        &mut method,
    )?;
    let method = &method[..method_len];
    let mut target = [0u8; 256];
    let target_len = str_slice(
        table,
        str_off,
        str_bytes,
        gw(table, W_TARGET_SOFF)?,
        gw(table, W_TARGET_SLEN)?,
        &mut target,
    )?;
    let target = &target[..target_len];
    let mut host = [0u8; 256];
    let host_len = str_slice(
        table,
        str_off,
        str_bytes,
        gw(table, W_HOST_SOFF)?,
        gw(table, W_HOST_SLEN)?,
        &mut host,
    )?;
    let host = &host[..host_len];

    // Claim well-formedness is concrete: the claims are public.
    if method.is_empty() || !method.iter().all(|&b| is_tchar(b as i32) == 1) {
        return Err("claimed method is not a token");
    }
    if method == b"HEAD" {
        // A HEAD response never has a body; this demo requires one.
        return Err("HEAD requests are out of scope");
    }
    if target.is_empty() || !target.iter().all(|&b| (0x21..=0x7E).contains(&b)) {
        return Err("claimed target is not printable ASCII");
    }

    // === request: method SP target SP HTTP/1.1 CRLF, headers, no body ===
    let method_end = gw(table, W_METHOD_END)?;
    let target_end = gw(table, W_TARGET_END)?;
    let req_head_end = gw(table, W_REQ_HEAD_END)?;
    if method_end != method.len() {
        return Err("method span/claim length mismatch");
    }
    if target_end != method_end + 1 + target.len() {
        return Err("target span/claim length mismatch");
    }
    // A request body is covered as OPAQUE private bytes: the demo claims
    // nothing about it, but its framing must still be unambiguous (below:
    // exactly one Content-Length whose digits equal the extent).
    if req_head_end > sent.len() {
        return Err("request head past the buffer");
    }
    let req_body_len = sent.len() - req_head_end;
    let req_line0 = target_end + 11; // SP "HTTP/1.1" CRLF
    // Shape first (concrete bounds), private byte checks only after.
    let req_n = gw(table, W_REQ_N_HDRS)?;
    let req_hs = shape_headers(
        table,
        gw(table, W_REQ_HDRS_OFF)?,
        req_n,
        req_line0,
        req_head_end,
        sent.len(),
    )?;
    check_eq_pub(sent, 0, method, &mut bad);
    check_lit(sent, method_end, b" ", &mut bad);
    check_eq_pub(sent, method_end + 1, target, &mut bad);
    check_lit(sent, target_end, b" HTTP/1.1\r\n", &mut bad);
    check_headers(sent, &req_hs, req_n, req_head_end, &mut bad);

    // Host binding: the claimed record is `Host` (case-insensitive), its
    // value equals the public claim, and no other header is `Host` (no
    // duplicates). Framing: with a body, the claimed record (and only it)
    // is `Content-Length` and its digits equal the body extent; without
    // one, no header may be `Content-Length`. `Transfer-Encoding` is out
    // of scope either way.
    let host_idx = gw(table, W_REQ_HOST_IDX)?;
    if host_idx >= req_n {
        return Err("host header index out of range");
    }
    if req_hs[host_idx].ve - req_hs[host_idx].vs != host.len() {
        return Err("host value span/claim length mismatch");
    }
    check_eq_pub(sent, req_hs[host_idx].vs, host, &mut bad);
    let req_cl_idx = gw(table, W_REQ_CL_IDX)?;
    if req_body_len > 0 {
        if req_cl_idx >= req_n {
            return Err("request content-length index out of range");
        }
        check_cl_digits(sent, &req_hs[req_cl_idx], req_body_len, &mut bad)?;
    } else if req_cl_idx != 0 {
        // Pinned so the word has no slack when it is meaningless.
        return Err("request content-length index must be 0 without a body");
    }
    for (i, h) in req_hs.iter().enumerate().take(req_n) {
        let m_host = name_eq_ci(sent, h.ns, h.ne, b"host");
        if i == host_idx {
            bad = bb(bad | f(m_host == 0));
        } else {
            bad = bb(bad | m_host);
        }
        let m_cl = name_eq_ci(sent, h.ns, h.ne, b"content-length");
        if req_body_len > 0 && i == req_cl_idx {
            bad = bb(bad | f(m_cl == 0));
        } else {
            bad = bb(bad | m_cl);
        }
        bad = bb(bad | name_eq_ci(sent, h.ns, h.ne, b"transfer-encoding"));
    }

    // === response: status line, headers, Content-Length framing ===
    let status = gw(table, W_STATUS)?;
    if !(100..=599).contains(&status) {
        return Err("status code out of range");
    }
    if (100..=199).contains(&status) || status == 204 || status == 304 {
        // Bodyless statuses can't carry the JSON body this demo requires.
        return Err("bodyless status is out of scope");
    }
    let reason_start = gw(table, W_REASON_START)?;
    let reason_end = gw(table, W_REASON_END)?;
    match reason_start {
        // No-reason form: CRLF directly after the code.
        12 => {
            if reason_end != 12 {
                return Err("no-reason form must have an empty reason");
            }
        }
        // `SP reason` form (the reason may still be empty).
        13 => {
            if reason_end < 13 {
                return Err("reason span inverted");
            }
        }
        _ => return Err("reason must start at 12 or 13"),
    }

    let resp_head_end = gw(table, W_RESP_HEAD_END)?;
    let resp_line0 = reason_end + 2;
    if resp_head_end >= recv.len() {
        return Err("response must have a body");
    }
    // Shape first (concrete bounds), private byte checks only after.
    let resp_n = gw(table, W_RESP_N_HDRS)?;
    let resp_hs = shape_headers(
        table,
        gw(table, W_RESP_HDRS_OFF)?,
        resp_n,
        resp_line0,
        resp_head_end,
        recv.len(),
    )?;
    check_lit(recv, 0, b"HTTP/1.1 ", &mut bad);
    let digits = [
        b'0' + (status / 100) as u8,
        b'0' + (status / 10 % 10) as u8,
        b'0' + (status % 10) as u8,
    ];
    check_lit(recv, 9, &digits, &mut bad);
    if reason_start == 13 {
        check_lit(recv, 12, b" ", &mut bad);
        for i in 13..reason_end {
            bad = bb(bad | f(is_value_byte(byte(recv, i)) == 0));
        }
    }
    check_lit(recv, reason_end, b"\r\n", &mut bad);
    check_headers(recv, &resp_hs, resp_n, resp_head_end, &mut bad);

    // Content-Length framing: the claimed record is `Content-Length`, its
    // digits equal the body extent, no other record is `Content-Length` or
    // `Transfer-Encoding` (so the derived framing is unambiguous), and at
    // most one `Host` (upstream rule).
    let body_len = recv.len() - resp_head_end;
    let cl_idx = gw(table, W_RESP_CL_IDX)?;
    if cl_idx >= resp_n {
        return Err("content-length index out of range");
    }
    check_cl_digits(recv, &resp_hs[cl_idx], body_len, &mut bad)?;
    let mut host_count = 0i32;
    for (i, h) in resp_hs.iter().enumerate().take(resp_n) {
        let m_cl = name_eq_ci(recv, h.ns, h.ne, b"content-length");
        if i == cl_idx {
            bad = bb(bad | f(m_cl == 0));
        } else {
            bad = bb(bad | m_cl);
        }
        bad = bb(bad | name_eq_ci(recv, h.ns, h.ne, b"transfer-encoding"));
        host_count = bb(host_count.wrapping_add(name_eq_ci(recv, h.ns, h.ne, b"host")));
    }
    bad = bb(bad | f(host_count >= 2));

    // === the claim: a JSON body value (the shared section, crate::json)
    // or one publicly-named header line ===
    let target = gw(table, W_CLAIM_TARGET)?;
    let (value_start, value_len, value_in_sent) = match target {
        TARGET_JSON => {
            // Pinned so the header-claim words have no slack.
            if gw(table, W_HDR_IDX)? != 0
                || gw(table, W_HDR_NAME_SOFF)? != 0
                || gw(table, W_HDR_NAME_SLEN)? != 0
            {
                return Err("header-claim words must be 0 for a JSON claim");
            }
            let n_nodes = gw(table, W_N_NODES)?;
            if n_nodes == 0 {
                return Err("the response body must be claimed as JSON");
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
            let (vs, vl) =
                verify_json_claim(recv, resp_head_end, recv.len(), table, &jc, &mut bad)?;
            (vs, vl, false)
        }
        TARGET_REQ_HEADER | TARGET_RESP_HEADER => {
            // The response body is covered as OPAQUE private bytes (its
            // framing was pinned above, contents unconstrained and
            // unclaimed); the JSON section must be empty, pinned so its
            // words have no slack.
            if gw(table, W_N_NODES)? != 0
                || gw(table, W_NODES_OFF)? != 0
                || gw(table, W_N_PATH)? != 0
                || gw(table, W_PATH_OFF)? != 0
                || gw(table, W_VALUE_NODE)? != 0
            {
                return Err("JSON-claim words must be 0 for a header claim");
            }
            let in_sent = target == TARGET_REQ_HEADER;
            let (hs, n, buf): (&[Hdr; MAX_HEADERS], usize, &[u8]) = if in_sent {
                (&req_hs, req_n, sent)
            } else {
                (&resp_hs, resp_n, recv)
            };
            let idx = gw(table, W_HDR_IDX)?;
            if idx >= n {
                return Err("claimed header index out of range");
            }
            let h = hs[idx];
            // The claimed name (public): a lowercase token whose private
            // bytes it must match case-insensitively. The span lengths are
            // both public, so a length mismatch is a concrete rejection.
            let mut name = [0u8; 256];
            let name_len = str_slice(
                table,
                str_off,
                str_bytes,
                gw(table, W_HDR_NAME_SOFF)?,
                gw(table, W_HDR_NAME_SLEN)?,
                &mut name,
            )?;
            let name = &name[..name_len];
            if name.is_empty() || !name.iter().all(|&b| is_tchar(b as i32) == 1) {
                return Err("claimed header name is not a token");
            }
            if name.iter().any(|b| b.is_ascii_uppercase()) {
                return Err("claimed header name must be lowercase");
            }
            if h.ne - h.ns != name.len() {
                return Err("claimed header name length mismatch");
            }
            bad = bb(bad | f(name_eq_ci(buf, h.ns, h.ne, name) == 0));

            let value_len = h.ve - h.vs;
            if value_len > OUT_CAP {
                return Err("claimed value too long");
            }
            let mode = gw(table, W_CLAIM_MODE)?;
            let expect_soff = gw(table, W_EXPECT_SOFF)?;
            let expect_slen = gw(table, W_EXPECT_SLEN)?;
            match mode {
                MODE_DISCLOSE => {
                    // Pinned so the words have no slack when meaningless.
                    if expect_soff != 0 || expect_slen != 0 {
                        return Err("expected string set in disclose mode");
                    }
                    (h.vs, value_len, in_sent)
                }
                MODE_ASSERT => {
                    if expect_slen == value_len {
                        let mut expect = [0u8; OUT_CAP];
                        let elen = str_slice(
                            table,
                            str_off,
                            str_bytes,
                            expect_soff,
                            expect_slen,
                            &mut expect,
                        )?;
                        check_eq_pub(buf, h.vs, &expect[..elen], &mut bad);
                    } else {
                        // Public lengths differ: the assert can never
                        // hold. Both parties fold the same rejection.
                        bad = bb(bad | 1);
                    }
                    (0, 0, false)
                }
                _ => return Err("unknown claim mode"),
            }
        }
        _ => return Err("unknown claim target"),
    };

    Ok(Disclosure {
        ok: f(bad == 0),
        value_start,
        value_len,
        value_in_sent,
    })
}

/// The (concrete) JSON-claim section of a flat table — node spans, path
/// components, claim mode — read out of the table by each format's own
/// header layout. The transcript table and the JSON-only table
/// ([`json`]) share this section's semantics, not its word positions.
pub(crate) struct JsonClaim {
    pub n_nodes: usize,
    pub nodes_off: usize,
    pub n_path: usize,
    pub path_off: usize,
    pub value_node: usize,
    pub str_off: usize,
    pub str_bytes: usize,
    pub mode: usize,
    pub expect_soff: usize,
    pub expect_slen: usize,
}

/// Verifies the claimed JSON value occupying `buf[body_start..body_end]`:
/// the (public) node spans drive a concrete walk, the private bytes are
/// checked at every claimed position, the gaps tile everything between,
/// the public path binds the claimed value node, and the claim mode is
/// applied. Every private byte check accumulates into `bad`; the returned
/// span is the (public) disclosure — `(0, 0)` in assert mode.
pub(crate) fn verify_json_claim(
    buf: &[u8],
    body_start: usize,
    body_end: usize,
    table: &[u32],
    jc: &JsonClaim,
    bad: &mut i32,
) -> Result<(usize, usize), &'static str> {
    let n_nodes = jc.n_nodes;
    let nodes_off = jc.nodes_off;

    let mut fr_node = [0usize; MAX_DEPTH];
    let mut fr_obj = [false; MAX_DEPTH];
    let mut fr_nkeys = [0usize; MAX_DEPTH];
    let mut fr_keys = [[0u32; MAX_KEYS]; MAX_DEPTH];
    let mut depth = 0usize;

    let mut p = body_start;
    let mut k = 0usize;

    'value: loop {
        if k >= n_nodes {
            return Err("node table exhausted");
        }
        let nd = get_node(table, nodes_off, k)?;
        if nd.start > nd.end || nd.end > body_end {
            return Err("node span out of bounds");
        }
        let fb = first_byte_pos(&nd)?;
        if fb < p {
            return Err("node spans out of order");
        }
        // Whitespace-only gap up to this value's first byte (separators are
        // consumed before re-entering the loop, below).
        check_gap(buf, p, fb, None, bad);
        p = fb;

        if nd.kind == KIND_OBJECT || nd.kind == KIND_ARRAY {
            let is_obj = nd.kind == KIND_OBJECT;
            if depth == MAX_DEPTH {
                return Err("JSON nesting too deep");
            }
            // At least the opening and closing byte.
            if nd.end < nd.start + 2 {
                return Err("container too small");
            }
            *bad = bb(*bad | f(eq(byte(buf, p), if is_obj { b'{' } else { b'[' }) == 0));
            fr_node[depth] = k;
            fr_obj[depth] = is_obj;
            fr_nkeys[depth] = 0;
            depth += 1;
            k += 1;
            p += 1; // past the opener
            if nd.size == 1 {
                // Empty container: the close cascade below checks the
                // whitespace up to the closer and the closer itself.
            } else if is_obj {
                // A member key must follow the brace.
                p = member_key(
                    buf, table, nodes_off, n_nodes, body_end, p, &mut k, &mut fr_keys,
                    &mut fr_nkeys, depth - 1, None, bad,
                )?;
                continue 'value;
            } else {
                continue 'value;
            }
        } else {
            // Scalar: the span must sit exactly at the cursor.
            let content_start = if nd.kind == KIND_STRING { p + 1 } else { p };
            if nd.start != content_start {
                return Err("scalar start mismatch");
            }
            if nd.size != 1 {
                return Err("leaf node size mismatch");
            }
            match nd.kind {
                KIND_STRING => {
                    if nd.end >= body_end {
                        return Err("string missing closing quote");
                    }
                    *bad = bb(*bad | f(eq(byte(buf, p), b'"') == 0));
                    check_string_content(buf, nd.start, nd.end, bad);
                    *bad = bb(*bad | f(eq(byte(buf, nd.end), b'"') == 0));
                    p = nd.end + 1;
                }
                KIND_NUMBER => {
                    if nd.end == nd.start || nd.end - nd.start > MAX_NUMBER_LEN {
                        return Err("number lexeme length out of range");
                    }
                    check_number(buf, nd.start, nd.end, bad);
                    p = nd.end;
                }
                KIND_BOOL => {
                    match nd.end - nd.start {
                        4 => check_lit(buf, nd.start, b"true", bad),
                        5 => check_lit(buf, nd.start, b"false", bad),
                        _ => return Err("bool lexeme length"),
                    }
                    p = nd.end;
                }
                KIND_NULL => {
                    if nd.end - nd.start != 4 {
                        return Err("null lexeme length");
                    }
                    check_lit(buf, nd.start, b"null", bad);
                    p = nd.end;
                }
                _ => return Err("unexpected node kind"),
            }
            k += 1;
        }

        // === close cascade: separators and container ends ===
        loop {
            if depth == 0 {
                // Root complete: trailing whitespace only, table consumed.
                check_gap(buf, p, body_end, None, bad);
                if k != n_nodes {
                    return Err("table has extra JSON nodes");
                }
                break 'value;
            }
            let fi = depth - 1;
            let fnode = get_node(table, nodes_off, fr_node[fi])?;
            let limit = fr_node[fi] + fnode.size;
            if limit > n_nodes {
                return Err("container size out of bounds");
            }
            if k < limit {
                // Another member/element: `ws* , ws*` up to its first byte.
                if fr_obj[fi] {
                    p = member_key(
                        buf, table, nodes_off, n_nodes, body_end, p, &mut k, &mut fr_keys,
                        &mut fr_nkeys, fi, Some(b','), bad,
                    )?;
                } else {
                    let nxt = get_node(table, nodes_off, k)?;
                    let fb = first_byte_pos(&nxt)?;
                    if fb < p {
                        return Err("node spans out of order");
                    }
                    check_gap(buf, p, fb, Some(b','), bad);
                    p = fb;
                }
                continue 'value;
            }
            // This container closes: whitespace to the closer byte.
            if fnode.end < p + 1 {
                return Err("container end before cursor");
            }
            check_gap(buf, p, fnode.end - 1, None, bad);
            *bad = bb(*bad
                | f(eq(byte(buf, fnode.end - 1), if fr_obj[fi] { b'}' } else { b']' }) == 0));
            p = fnode.end;
            // Duplicate keys: pairwise raw comparison (same-length pairs
            // only — the lengths are public).
            let nk = fr_nkeys[fi];
            for a in 0..nk {
                for b2 in a + 1..nk {
                    let ka = get_node(table, nodes_off, fr_keys[fi][a] as usize)?;
                    let kb = get_node(table, nodes_off, fr_keys[fi][b2] as usize)?;
                    if ka.end - ka.start == kb.end - kb.start {
                        *bad =
                            bb(*bad | bytes_eq_priv(buf, ka.start, kb.start, ka.end - ka.start));
                    }
                }
            }
            depth -= 1;
        }
    }

    // === the path claim: public components bind the disclosed node ===
    let n_path = jc.n_path;
    let path_off = jc.path_off;
    if n_path > MAX_PATH {
        return Err("too many path components");
    }
    if n_path == 0 && path_off != 0 {
        // Pinned so the word has no slack when it is meaningless.
        return Err("path offset must be 0 without components");
    }
    let mut cur = 0usize; // current value node (the root)
    for c in 0..n_path {
        let base = path_off + 4 * c;
        let is_index = gw(table, base)?;
        let idx = gw(table, base + 1)?;
        let soff = gw(table, base + 2)?;
        let slen = gw(table, base + 3)?;
        let nd = get_node(table, nodes_off, cur)?;
        if is_index == 0 {
            // Key component: `idx` must be a direct key child of `cur`,
            // and its content must equal the public component bytes. The
            // duplicate-key rejection above makes the match unique.
            if nd.kind != KIND_OBJECT {
                return Err("path key component on a non-object");
            }
            let limit = cur + nd.size;
            let mut child = cur + 1;
            let mut found = false;
            while child < limit {
                if child == idx {
                    found = true;
                }
                let vsub = child + 1;
                if vsub >= limit {
                    return Err("object member without value");
                }
                child = vsub + get_node(table, nodes_off, vsub)?.size;
            }
            if !found {
                return Err("path key index is not a member of its object");
            }
            let key = get_node(table, nodes_off, idx)?;
            if key.end - key.start != slen || slen > 64 {
                return Err("path component length mismatch");
            }
            let mut comp = [0u8; 64];
            let clen = str_slice(table, jc.str_off, jc.str_bytes, soff, slen, &mut comp)?;
            check_eq_pub(buf, key.start, &comp[..clen], bad);
            cur = idx + 1;
        } else {
            // Array-index component: pure public tree navigation.
            if nd.kind != KIND_ARRAY {
                return Err("path index component on a non-array");
            }
            let limit = cur + nd.size;
            let mut child = cur + 1;
            for _ in 0..idx {
                if child >= limit {
                    return Err("array index out of range");
                }
                child += get_node(table, nodes_off, child)?.size;
            }
            if child >= limit {
                return Err("array index out of range");
            }
            cur = child;
        }
    }
    if cur != jc.value_node {
        return Err("path does not resolve to the claimed node");
    }
    let value = get_node(table, nodes_off, cur)?;
    match value.kind {
        KIND_NULL | KIND_BOOL | KIND_NUMBER | KIND_STRING => {}
        _ => return Err("disclosed value must be a scalar"),
    }
    let value_len = value.end - value.start;
    if value_len > OUT_CAP {
        return Err("claimed value too long");
    }

    // The claim itself: disclose the value's bytes, or assert (privately)
    // that they equal the public expected string.
    let mode = jc.mode;
    let expect_soff = jc.expect_soff;
    let expect_slen = jc.expect_slen;
    let (value_start, out_len) = match mode {
        MODE_DISCLOSE => {
            // Pinned so the words have no slack when they are meaningless.
            if expect_soff != 0 || expect_slen != 0 {
                return Err("expected string set in disclose mode");
            }
            (value.start, value_len)
        }
        MODE_ASSERT => {
            if expect_slen == value_len {
                let mut expect = [0u8; OUT_CAP];
                let elen = str_slice(
                    table,
                    jc.str_off,
                    jc.str_bytes,
                    expect_soff,
                    expect_slen,
                    &mut expect,
                )?;
                check_eq_pub(buf, value.start, &expect[..elen], bad);
            } else {
                // Public lengths differ: the assert can never hold. Both
                // parties fold the same concrete rejection.
                *bad = bb(*bad | 1);
            }
            (0, 0)
        }
        _ => return Err("unknown claim mode"),
    };

    Ok((value_start, out_len))
}

/// Verifies one object member key at the cursor and the colon gap to its
/// value; returns the cursor at the value's first byte and advances `k`
/// past the key node. `sep` is the pending separator before the key
/// (`None` right after `{`, `,` between members).
#[allow(clippy::too_many_arguments)]
fn member_key(
    recv: &[u8],
    table: &[u32],
    nodes_off: usize,
    n_nodes: usize,
    body_end: usize,
    p: usize,
    k: &mut usize,
    fr_keys: &mut [[u32; MAX_KEYS]; MAX_DEPTH],
    fr_nkeys: &mut [usize; MAX_DEPTH],
    fi: usize,
    sep: Option<u8>,
    bad: &mut i32,
) -> Result<usize, &'static str> {
    if *k >= n_nodes {
        return Err("node table exhausted");
    }
    let key = get_node(table, nodes_off, *k)?;
    if key.kind != KIND_KEY {
        return Err("expected a key node");
    }
    if key.size != 1 {
        return Err("key node size mismatch");
    }
    if key.start > key.end || key.end >= body_end {
        return Err("key span out of bounds");
    }
    let kq = key.start.checked_sub(1).ok_or("key span at 0")?;
    if kq < p {
        return Err("key span out of order");
    }
    check_gap(recv, p, kq, sep, bad);
    *bad = bb(*bad | f(eq(byte(recv, kq), b'"') == 0));
    check_string_content(recv, key.start, key.end, bad);
    *bad = bb(*bad | f(eq(byte(recv, key.end), b'"') == 0));
    if fr_nkeys[fi] == MAX_KEYS {
        return Err("too many keys in one object");
    }
    fr_keys[fi][fr_nkeys[fi]] = *k as u32;
    fr_nkeys[fi] += 1;
    *k += 1;
    // The member value is the next node; the gap to it holds the colon.
    if *k >= n_nodes {
        return Err("object member without value node");
    }
    let vn = get_node(table, nodes_off, *k)?;
    let vfb = first_byte_pos(&vn)?;
    if vfb < key.end + 1 {
        return Err("member value before its key");
    }
    check_gap(recv, key.end + 1, vfb, Some(b':'), bad);
    Ok(vfb)
}

#[cfg(test)]
mod json_tests;
#[cfg(test)]
mod tests;
