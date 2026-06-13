//! ECDSA verification for the zk-vm, over a toy 64-bit curve.
//!
//! The statement the guest proves: "I hold a message and an ECDSA signature
//! over it by the (public) key Q" — revealing neither the message nor the
//! signature, only the 0/1 verdict. The verification is the real ECDSA
//! algorithm — SHA-256(msg) → u₁·G + u₂·Q → x ≟ r — run branch-free inside
//! the VM over **private** (r, s) and message bytes.
//!
//! ## The curve (toy, on purpose)
//!
//! `y² = x³ + 2` over `p = 2^64 − 453`: a j = 0 curve — secp256k1's little
//! sibling — with prime group order `n = 2^64 − 7386677115` (cofactor 1, so
//! every curve point lies in the group and no point has order 2). A 64-bit
//! curve offers no real security (Pollard rho breaks it in ~2^32 steps); it
//! is sized so the in-VM verification costs single-digit seconds. The same
//! code at P-256 width needs ~100× the multiplications — upstream VM work
//! (native bignums or RAM) before that's demo-viable. Parameters were found
//! by a one-off search (see `tests/gen.rs`): the j = 0 family has CM by
//! √−3, so group orders come in closed form from 4p = L² + 27M² — no
//! point counting needed — and are verified here rigorously (a prime n in
//! the Hasse interval with n·P = O for random points P fixes #E = n).
//!
//! ## How it fits the VM
//!
//! The zk-vm cannot branch on, index by, or `select` on private data, so the
//! expensive and data-dependent parts are restructured with the **advice
//! pattern** (as in the transcript guest): the prover computes hints OFF the
//! VM and the guest only *checks* them, branch-free:
//!
//! - **Inverses as advice.** Affine point formulas need 1/(x₂−x₁) and
//!   1/(2y). Computing an inverse in-VM costs a ~96-multiplication Fermat
//!   ladder; checking one costs a single multiplication: the prover supplies
//!   `inv` privately and the guest pins `denom · inv = 1` into the verdict.
//!   Bogus advice can only flip the verdict to 0, never to 1 — the algebra
//!   over a prime field forces `inv` exactly.
//! - **s⁻¹ as advice**, checked the same way (`s·w ≡ 1 mod n`).
//! - **Fixed-window comb for u₁·G + u₂·Q.** Both parties derive, off the VM,
//!   a public 256-entry table per base (all 8-bit row-combinations of the
//!   scalar's comb decomposition). The walk is 8 doublings + 16 additions of
//!   table entries selected by masked merge — a public-index scan, never a
//!   private index. Table entries carry fixed offsets (Dg, Dq) and the
//!   accumulator seeds at T so no honest entry or intermediate is the point
//!   at infinity (unrepresentable in affine coordinates); one final addition
//!   of the public correction point C = −(256·T + 255·(Dg+Dq)) removes the
//!   offsets. A degenerate addition (x₂ = x₁) has no valid advice — the
//!   inverse check fails and the proof honestly outputs 0.
//!
//! Both moduli are pseudo-Mersenne (2^64 − small c), so reduction is two
//! fold-and-add rounds plus one conditional subtraction — no Montgomery
//! domain, no division.
//!
//! The `host` module (feature `host`, never linked into the guest's VM path)
//! holds the branching counterparts: signing, table construction, and advice
//! generation — the latter replays the *same* `walk` as the in-VM verifier,
//! so the two cannot drift.

// ── curve & demo-key parameters (see tests/gen.rs for provenance) ──

/// Field prime `p = 2^64 − 453`.
pub const P: u64 = 0xffff_ffff_ffff_fe3b;
/// `2^64 − P`.
pub const C_P: u64 = 453;
/// Group order `n = 2^64 − 7386677115` (prime; cofactor 1).
pub const N: u64 = 0xffff_fffe_47b8_4085;
/// `2^64 − N`.
pub const C_N: u64 = 7_386_677_115;
/// Curve coefficient: `y² = x³ + B`.
pub const B: u64 = 2;
/// Base point.
pub const GX: u64 = 0x2;
pub const GY: u64 = 0x150d_1538_c05f_caed;
/// Demo signing key, ⌊e·10^18⌋ mod n. Embedded on purpose: the demo signs
/// in the prover's worker and proves verification in the VM — the key is a
/// prop, not a secret.
pub const D_DEMO: u64 = 0x25b9_46eb_c0b3_6173;
/// The demo public key `Q = D_DEMO·G`.
pub const QX: u64 = 0xe7c6_8dd6_9786_cb33;
pub const QY: u64 = 0x9b9c_0552_8c30_4395;
/// Comb-walk system points: the accumulator seed T and per-table offsets
/// Dg, Dq (any fixed valid points work — soundness never rests on their
/// discrete logs; they only keep affine representations away from infinity).
pub const TX: u64 = 0x1003;
pub const TY: u64 = 0x2f88_7758_6667_12ac;
pub const DGX: u64 = 0x2000;
pub const DGY: u64 = 0x7aae_64f4_e3a7_7db6;
pub const DQX: u64 = 0x3002;
pub const DQY: u64 = 0x6ff1_c011_1052_d5b8;

// ── guest memory layout ──

/// Demo cap on the message size (proving cost grows with SHA-256 blocks).
pub const MSG_CAP: usize = 512;
/// Private advice words: r, s, w = s⁻¹ mod n, then one field inverse per
/// walk step (8 doublings + 16 table additions + 1 correction addition).
pub const ADVICE_WORDS: usize = 3 + STEPS * 3 + 1;
pub const ADVICE_BYTES: usize = ADVICE_WORDS * 8;
/// Public table words: 256 (x, y) entries per base, then T and C.
pub const TABLE_WORDS: usize = 256 * 2 * 2 + 4;
pub const TABLE_BYTES: usize = TABLE_WORDS * 8;
/// Comb columns: 64-bit scalars as 8 teeth × 8 columns.
const STEPS: usize = 8;
/// Word offsets inside the table region.
const TQ_OFF: usize = 512;
const T_OFF: usize = 1024;
const C_OFF: usize = 1026;

// ── branch-free field arithmetic (mod 2^64 − c) ──
//
// Flag discipline per the lowering traps in CLAUDE.md: every comparison-
// derived 0/1 value is pinned with `black_box` the moment it is created, so
// LLVM can never prove a {0,1} range and lower `0 − flag` or `flag · c`
// back into a (symbolic) `select`.

#[inline(always)]
fn bb(x: u64) -> u64 {
    core::hint::black_box(x)
}

#[inline(always)]
fn flag(b: bool) -> u64 {
    bb(b as u64)
}

/// All-ones mask from a pinned 0/1 flag.
#[inline(always)]
fn mask(f: u64) -> u64 {
    bb(0u64.wrapping_sub(f))
}

/// Full 64×64 → 128-bit product as (hi, lo), via 32-bit halves — wasm32 has
/// no widening multiply, and `u128` would pull in a compiler-rt libcall.
fn mul_wide(a: u64, b: u64) -> (u64, u64) {
    let (a0, a1) = (a & 0xffff_ffff, a >> 32);
    let (b0, b1) = (b & 0xffff_ffff, b >> 32);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let (mid, mid_c) = p01.overflowing_add(p10);
    let (lo, lo_c) = p00.overflowing_add(mid << 32);
    // p11 + carries < 2^64: the three partial products can't fill the top.
    let hi = p11 + (mid >> 32) + (flag(mid_c) << 32) + flag(lo_c);
    (hi, lo)
}

/// One conditional subtraction: x reduced into [0, m) for m = 2^64 − c,
/// valid for any x < 2m (true for every u64, since m > 2^63 here).
fn cond_sub(x: u64, c: u64) -> u64 {
    let m = 0u64.wrapping_sub(c);
    let (d, borrow) = x.overflowing_sub(m);
    d.wrapping_add(m & mask(flag(borrow)))
}

/// Reduces hi·2^64 + lo modulo m = 2^64 − c, for c < 2^34: fold the high
/// word down with hi·2^64 ≡ hi·c twice (bounds shrink to a few bits), then
/// one last fold and one conditional subtraction.
fn reduce(hi: u64, lo: u64, c: u64) -> u64 {
    let (h1, l1) = mul_wide(hi, c); // h1 < c ≤ 2^34
    let (s1, c1) = lo.overflowing_add(l1);
    let h2 = h1 + flag(c1); // ≤ 2^34
    let (h3, l3) = mul_wide(h2, c); // h3 ≤ 2^4
    let (s2, c2) = s1.overflowing_add(l3);
    let h4 = h3 + flag(c2); // ≤ 2^4 + 1
    let (s3, c3) = s2.overflowing_add(h4 * c); // h4·c < 2^39
    // If that carried, s3 < 2^39, so one more c fits without carrying.
    let s4 = s3 + flag(c3) * c;
    cond_sub(s4, c)
}

/// a·b mod (2^64 − c). Operands may be any u64 (advice words arrive
/// unreduced); the result is canonical.
pub fn mod_mul(a: u64, b: u64, c: u64) -> u64 {
    let (hi, lo) = mul_wide(a, b);
    reduce(hi, lo, c)
}

/// a + b mod (2^64 − c), for canonical a, b.
fn mod_add(a: u64, b: u64, c: u64) -> u64 {
    let (s, carry) = a.overflowing_add(b);
    // a + b < 2m, so a wrapped sum is < 2^64 − 2c and adding c can't carry.
    cond_sub(s + flag(carry) * c, c)
}

/// a − b mod (2^64 − c), for canonical a, b.
fn mod_sub(a: u64, b: u64, c: u64) -> u64 {
    let m = 0u64.wrapping_sub(c);
    let (d, borrow) = a.overflowing_sub(b);
    d.wrapping_add(m & mask(flag(borrow)))
}

// ── SHA-256 (branch-free: all control flow depends on the public length) ──
//
// Same algorithm as the sha256 guest, restated over slices so the host-side
// signer and the in-VM verifier share one implementation.

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for t in 0..16 {
        w[t] = u32::from_be_bytes([block[4 * t], block[4 * t + 1], block[4 * t + 2], block[4 * t + 3]]);
    }
    for t in 16..64 {
        let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
        let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for t in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

/// Runs the SHA-256 block loop over a `len`-byte message read through
/// `byte`, accumulating into `h` (which the caller seeds with `H0`).
///
/// A byte accessor instead of a `&[u8]`: the guest reads the message
/// straight from its authenticated VM buffer, so no message array is
/// materialized on the stack. `block` is a small local fully overwritten
/// every iteration (the one zk-vm-safe array shape — unlike a returned or
/// moved array, which lowers to `memory.copy` and faults; see `verify`).
fn sha256_blocks(h: &mut [u32; 8], len: usize, byte: impl Fn(usize) -> u8) {
    let bitlen = (len as u64).wrapping_mul(8);
    let mut total = len + 9;
    if total % 64 != 0 {
        total += 64 - total % 64;
    }
    let mut block = [0u8; 64];
    let mut pos = 0;
    while pos < total {
        for (i, slot) in block.iter_mut().enumerate() {
            let idx = pos + i;
            *slot = if idx < len {
                byte(idx)
            } else if idx == len {
                0x80
            } else if idx + 8 >= total {
                (bitlen >> ((total - 1 - idx) * 8)) as u8
            } else {
                0
            };
        }
        compress(h, &block);
        pos += 64;
    }
}

pub fn sha256(msg: &[u8]) -> [u8; 32] {
    let mut h = H0;
    sha256_blocks(&mut h, msg.len(), |i| msg[i]);
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

/// The ECDSA hash scalar: the leftmost 64 bits of SHA-256(msg) (FIPS 186
/// truncation), reduced mod n (one conditional subtraction, n > 2^63).
/// Returns a scalar — no digest array escapes — so the in-VM path needs no
/// stack array.
fn e_scalar(len: usize, byte: impl Fn(usize) -> u8) -> u64 {
    let mut h = H0;
    sha256_blocks(&mut h, len, byte);
    cond_sub(((h[0] as u64) << 32) | (h[1] as u64), C_N)
}

// ── the branch-free verifier ──

/// Comb column j of a 64-bit scalar: bits {j, j+8, …, j+56} packed into a
/// byte (constant shift amounts only).
fn window(u: u64, j: usize) -> u64 {
    let mut w = 0u64;
    for k in 0..8 {
        w |= ((u >> (8 * k + j)) & 1) << k;
    }
    w
}

/// Constant-scan select: the entry at (private) index `w` of the 256-entry
/// (x, y) sub-table starting at word `base`, as a masked merge over every
/// entry — a public loop, never a private index. Words are read through
/// `table_word` so the guest scans its authenticated VM buffer in place.
fn select256(table_word: &impl Fn(usize) -> u64, base: usize, w: u64) -> (u64, u64) {
    let mut x = 0u64;
    let mut y = 0u64;
    for i in 0..256 {
        let m = mask(flag(w == i as u64));
        x ^= table_word(base + 2 * i) & m;
        y ^= table_word(base + 2 * i + 1) & m;
    }
    (x, y)
}

/// Affine doubling with an advice inverse: `denom` must be 2·y₁ and `inv`
/// its claimed inverse; the check lands in `bad`.
fn double_step(ax: u64, ay: u64, denom: u64, inv: u64, bad: &mut u64) -> (u64, u64) {
    let chk = mod_mul(denom, inv, C_P);
    *bad = bb(*bad | bb(chk ^ 1));
    let xx = mod_mul(ax, ax, C_P);
    let lam = mod_mul(mod_add(mod_add(xx, xx, C_P), xx, C_P), inv, C_P);
    let x3 = mod_sub(mod_sub(mod_mul(lam, lam, C_P), ax, C_P), ax, C_P);
    let y3 = mod_sub(mod_mul(lam, mod_sub(ax, x3, C_P), C_P), ay, C_P);
    (x3, y3)
}

/// Affine addition of (ex, ey) with an advice inverse: `denom` must be
/// ex − ax and `inv` its claimed inverse.
fn add_step(ax: u64, ay: u64, ex: u64, ey: u64, denom: u64, inv: u64, bad: &mut u64) -> (u64, u64) {
    let chk = mod_mul(denom, inv, C_P);
    *bad = bb(*bad | bb(chk ^ 1));
    let lam = mod_mul(mod_sub(ey, ay, C_P), inv, C_P);
    let x3 = mod_sub(mod_sub(mod_mul(lam, lam, C_P), ax, C_P), ex, C_P);
    let y3 = mod_sub(mod_mul(lam, mod_sub(ax, x3, C_P), C_P), ay, C_P);
    (x3, y3)
}

/// The comb walk for u₁·G + u₂·Q: seed at T, then per column double once
/// and add one selected entry from each base's table, then add the public
/// correction C. Returns the result's x coordinate. `inv_for(denom)`
/// supplies the inverse for each step — the verifier feeds advice words,
/// the host-side advice generator computes (and records) real inverses, so
/// both sides replay the identical sequence.
fn walk(
    u1: u64,
    u2: u64,
    table_word: &impl Fn(usize) -> u64,
    mut inv_for: impl FnMut(u64) -> u64,
    bad: &mut u64,
) -> u64 {
    let (mut ax, mut ay) = (table_word(T_OFF), table_word(T_OFF + 1));
    for j in (0..STEPS).rev() {
        let denom = mod_add(ay, ay, C_P);
        let inv = inv_for(denom);
        (ax, ay) = double_step(ax, ay, denom, inv, bad);
        for (base, u) in [(0usize, u1), (TQ_OFF, u2)] {
            let (ex, ey) = select256(table_word, base, window(u, j));
            let denom = mod_sub(ex, ax, C_P);
            let inv = inv_for(denom);
            (ax, ay) = add_step(ax, ay, ex, ey, denom, inv, bad);
        }
    }
    let (cx, cy) = (table_word(C_OFF), table_word(C_OFF + 1));
    let denom = mod_sub(cx, ax, C_P);
    let inv = inv_for(denom);
    let (rx, _) = add_step(ax, ay, cx, cy, denom, inv, bad);
    rx
}

/// Verifies an ECDSA signature branch-free, reading every input through an
/// accessor: `msg_byte(i)` the i-th message byte, `table_word(i)` the i-th
/// public comb-table word (both parties derive the table from Q off the
/// VM), `advice_word(i)` the i-th private advice word
/// ([r, s, s⁻¹ mod n, step inverses…]). Accessors — not slices — so the
/// guest reads straight from its authenticated VM buffers: a `&[u8]`/`&[u64]`
/// would force the guest to copy the inputs into a stack array, and a large
/// array move lowers to `memory.copy`, whose bytes the zk-vm never
/// authenticates (it would fault with `MemAuthMissing` on read-back).
///
/// Returns the 0/1 verdict; every internal check only ever accumulates into
/// it — bogus advice can flip a 1 to a 0, never the reverse.
pub fn verify(
    msg_len: usize,
    msg_byte: impl Fn(usize) -> u8,
    table_word: impl Fn(usize) -> u64,
    advice_word: impl Fn(usize) -> u64,
) -> u32 {
    let (r, s, w) = (advice_word(0), advice_word(1), advice_word(2));
    let mut bad = 0u64;
    // Signature range: 1 ≤ r, s < n.
    bad |= flag(r == 0) | flag(s == 0) | flag(r >= N) | flag(s >= N);
    // w is forced to s⁻¹ mod n by the prime field.
    bad |= bb(mod_mul(s, w, C_N) ^ 1);
    let e = e_scalar(msg_len, &msg_byte);
    let u1 = mod_mul(e, w, C_N);
    let u2 = mod_mul(r, w, C_N);
    let mut k = 3usize;
    let rx = walk(
        u1,
        u2,
        &table_word,
        |_denom| {
            let inv = advice_word(k);
            k += 1;
            inv
        },
        &mut bad,
    );
    // x of u₁·G + u₂·Q, mod n, must equal r. rx < p < 2n, so one
    // conditional subtraction canonicalizes; both sides then compare as
    // plain words.
    bad |= bb(cond_sub(rx, C_N) ^ r);
    flag(bb(bad) == 0) as u32
}

/// Slice-backed [`verify`] for host code and native tests (where stack
/// arrays are free): reads the message, table, and advice from `&[…]`.
pub fn verify_slices(msg: &[u8], table: &[u64], advice: &[u64]) -> u32 {
    assert_eq!(table.len(), TABLE_WORDS);
    assert_eq!(advice.len(), ADVICE_WORDS);
    verify(msg.len(), |i| msg[i], |i| table[i], |i| advice[i])
}

// ── host-side helpers: signing, tables, advice (never inside the VM) ──

#[cfg(any(test, feature = "host"))]
pub mod host {
    use super::*;

    /// Affine point; `None` is the point at infinity.
    pub type Point = Option<(u64, u64)>;

    fn mulmod(a: u64, b: u64, m: u64) -> u64 {
        (a as u128 * b as u128 % m as u128) as u64
    }

    fn addmod(a: u64, b: u64, m: u64) -> u64 {
        ((a as u128 + b as u128) % m as u128) as u64
    }

    fn submod(a: u64, b: u64, m: u64) -> u64 {
        ((a as u128 + m as u128 - b as u128) % m as u128) as u64
    }

    fn powmod(mut b: u64, mut e: u64, m: u64) -> u64 {
        let mut r = 1u64;
        b %= m;
        while e > 0 {
            if e & 1 == 1 {
                r = mulmod(r, b, m);
            }
            b = mulmod(b, b, m);
            e >>= 1;
        }
        r
    }

    /// a⁻¹ mod prime m via Fermat; 0 maps to 0 (the "no inverse exists"
    /// advice that makes the in-VM check fail honestly).
    pub fn modinv(a: u64, m: u64) -> u64 {
        powmod(a, m - 2, m)
    }

    pub fn on_curve(x: u64, y: u64) -> bool {
        mulmod(y, y, P) == addmod(mulmod(x, mulmod(x, x, P), P), B, P)
    }

    pub fn padd(p1: Point, p2: Point) -> Point {
        let (x1, y1) = match p1 {
            None => return p2,
            Some(q) => q,
        };
        let (x2, y2) = match p2 {
            None => return p1,
            Some(q) => q,
        };
        let lam = if x1 == x2 {
            if addmod(y1, y2, P) == 0 {
                return None;
            }
            let xx = mulmod(x1, x1, P);
            mulmod(addmod(addmod(xx, xx, P), xx, P), modinv(addmod(y1, y1, P), P), P)
        } else {
            mulmod(submod(y2, y1, P), modinv(submod(x2, x1, P), P), P)
        };
        let x3 = submod(submod(mulmod(lam, lam, P), x1, P), x2, P);
        let y3 = submod(mulmod(lam, submod(x1, x3, P), P), y1, P);
        Some((x3, y3))
    }

    pub fn pmul(mut k: u64, mut g: Point) -> Point {
        let mut acc: Point = None;
        while k > 0 {
            if k & 1 == 1 {
                acc = padd(acc, g);
            }
            g = padd(g, g);
            k >>= 1;
        }
        acc
    }

    pub fn pneg(p: Point) -> Point {
        p.map(|(x, y)| (x, submod(0, y, P)))
    }

    /// The hash-derived scalar e, exactly as the in-VM verifier derives it.
    pub fn msg_scalar(msg: &[u8]) -> u64 {
        u64::from_be_bytes(sha256(msg)[..8].try_into().unwrap()) % N
    }

    /// Signs with a deterministic nonce (RFC 6979 in spirit: k from
    /// SHA-256(d ‖ H(m) ‖ counter) — demo-grade derivation).
    pub fn sign(d: u64, msg: &[u8]) -> (u64, u64) {
        let e = msg_scalar(msg);
        let digest = sha256(msg);
        let mut ctr = 0u64;
        loop {
            let mut buf = [0u8; 48];
            buf[..8].copy_from_slice(&d.to_be_bytes());
            buf[8..40].copy_from_slice(&digest);
            buf[40..].copy_from_slice(&ctr.to_be_bytes());
            ctr += 1;
            let k = u64::from_be_bytes(sha256(&buf)[..8].try_into().unwrap()) % N;
            if k == 0 {
                continue;
            }
            let Some((rx, _)) = pmul(k, Some((GX, GY))) else {
                continue;
            };
            let r = rx % N;
            if r == 0 {
                continue;
            }
            let s = mulmod(modinv(k, N), addmod(e, mulmod(r, d, N), N), N);
            if s == 0 {
                continue;
            }
            return (r, s);
        }
    }

    /// Textbook ECDSA verification — the independent reference the
    /// branch-free verifier is differentially tested against.
    pub fn native_verify(q: (u64, u64), msg: &[u8], r: u64, s: u64) -> bool {
        if r == 0 || r >= N || s == 0 || s >= N {
            return false;
        }
        let e = msg_scalar(msg);
        let w = modinv(s, N);
        let u1 = mulmod(e, w, N);
        let u2 = mulmod(r, w, N);
        match padd(pmul(u1, Some((GX, GY))), pmul(u2, Some(q))) {
            None => false,
            Some((x, _)) => x % N == r,
        }
    }

    /// The public comb table for bases G and Q: per base, entry w is
    /// (Σ_{k: bit k of w} 2^(8k)·base) + offset, then the seed T and the
    /// correction C = −(256·T + 255·(Dg + Dq)). Both parties compute this
    /// independently from public data; the protocol checks the copies agree.
    pub fn tables(q: (u64, u64)) -> Vec<u64> {
        let mut out = Vec::with_capacity(TABLE_WORDS);
        for (base, off) in [((GX, GY), (DGX, DGY)), (q, (DQX, DQY))] {
            let mut pows = [Some(base); 8];
            for k in 1..8 {
                pows[k] = pmul(256, pows[k - 1]);
            }
            let mut tab: Vec<Point> = vec![Some(off); 256];
            for w in 1..256usize {
                tab[w] = padd(tab[w & (w - 1)], pows[w.trailing_zeros() as usize]);
            }
            for entry in tab {
                let (x, y) = entry.expect("table entry degenerated to infinity");
                out.push(x);
                out.push(y);
            }
        }
        out.push(TX);
        out.push(TY);
        let drift = padd(
            pmul(256, Some((TX, TY))),
            pmul(255, padd(Some((DGX, DGY)), Some((DQX, DQY)))),
        );
        let (cx, cy) = pneg(drift).expect("correction degenerated to infinity");
        out.push(cx);
        out.push(cy);
        out
    }

    /// The prover's private advice for one run: r, s, w = s⁻¹ mod n, then
    /// the field inverse for every walk step — produced by replaying the
    /// exact in-VM `walk`, so the sequences cannot drift. Works for invalid
    /// signatures too (the walk is still well-defined; the verdict check
    /// fails honestly), recording 0 where no inverse exists.
    pub fn advice(table: &[u64], msg: &[u8], r: u64, s: u64) -> [u64; ADVICE_WORDS] {
        let mut adv = [0u64; ADVICE_WORDS];
        adv[0] = r;
        adv[1] = s;
        let w = modinv(s % N, N);
        adv[2] = w;
        let e = msg_scalar(msg);
        let u1 = mulmod(e, w, N);
        let u2 = mulmod(r % N, w, N);
        let mut k = 3;
        let mut bad = 0u64;
        walk(
            u1,
            u2,
            &|i| table[i],
            |denom| {
                let inv = modinv(denom, P);
                adv[k] = inv;
                k += 1;
                inv
            },
            &mut bad,
        );
        adv
    }
}

#[cfg(test)]
mod tests {
    use super::host::*;
    use super::*;

    /// Deterministic Miller-Rabin, valid for all u64.
    fn is_prime(n: u64) -> bool {
        let mulmod = |a: u64, b: u64| (a as u128 * b as u128 % n as u128) as u64;
        let powmod = |mut b: u64, mut e: u64| {
            let mut r = 1u64;
            while e > 0 {
                if e & 1 == 1 {
                    r = mulmod(r, b);
                }
                b = mulmod(b, b);
                e >>= 1;
            }
            r
        };
        if n < 2 {
            return false;
        }
        for p in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
            if n % p == 0 {
                return n == p;
            }
        }
        let s = (n - 1).trailing_zeros();
        let d = (n - 1) >> s;
        'witness: for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
            let mut x = powmod(a, d);
            if x == 1 || x == n - 1 {
                continue;
            }
            for _ in 0..s - 1 {
                x = mulmod(x, x);
                if x == n - 1 {
                    continue 'witness;
                }
            }
            return false;
        }
        true
    }

    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn parameters_are_consistent() {
        assert!(is_prime(P));
        assert!(is_prime(N));
        assert_eq!(P, 0u64.wrapping_sub(C_P));
        assert_eq!(N, 0u64.wrapping_sub(C_N));
        assert_eq!(P % 3, 1); // j = 0 needs an ordinary curve
        for (x, y) in [(GX, GY), (QX, QY), (TX, TY), (DGX, DGY), (DQX, DQY)] {
            assert!(on_curve(x, y), "({x:#x}, {y:#x}) is off the curve");
        }
        // n really is the group order (prime in the Hasse interval), and
        // the demo key matches.
        assert_eq!(pmul(N, Some((GX, GY))), None);
        assert_eq!(pmul(D_DEMO, Some((GX, GY))), Some((QX, QY)));
    }

    #[test]
    fn field_ops_match_u128_reference() {
        let mut st = 0x1234_5678_9abc_def0u64;
        let edge = [0u64, 1, 2, C_P, C_N, P - 1, N - 1, P, N, u64::MAX];
        let mut values: Vec<u64> = edge.to_vec();
        for _ in 0..200 {
            values.push(xorshift(&mut st));
        }
        for c in [C_P, C_N] {
            let m = 0u64.wrapping_sub(c) as u128;
            for &a in &values {
                for &b in &values {
                    assert_eq!(
                        mod_mul(a, b, c) as u128,
                        a as u128 * b as u128 % m,
                        "mod_mul({a:#x}, {b:#x}) mod 2^64-{c}"
                    );
                    if (a as u128) < m && (b as u128) < m {
                        assert_eq!(
                            mod_add(a, b, c) as u128,
                            (a as u128 + b as u128) % m,
                            "mod_add({a:#x}, {b:#x})"
                        );
                        assert_eq!(
                            mod_sub(a, b, c) as u128,
                            (a as u128 + m - b as u128) % m,
                            "mod_sub({a:#x}, {b:#x})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sha256_matches_test_vectors() {
        let hex = |d: [u8; 32]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // 56 bytes: exercises the two-block padding boundary.
        assert_eq!(
            hex(sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(sha256(&[0xab; 1024])),
            "4555555dc68d872c2270ba89ecc5f6f094812f65372b37e50071fe5168031c49"
        );
    }

    #[test]
    fn comb_tables_are_valid_points() {
        let table = tables((QX, QY));
        assert_eq!(table.len(), TABLE_WORDS);
        for pair in table.chunks_exact(2) {
            assert!(on_curve(pair[0], pair[1]));
        }
    }

    /// The walk really computes u₁·G + u₂·Q once the offsets cancel.
    #[test]
    fn walk_matches_native_linear_combination() {
        let table = tables((QX, QY));
        let mut st = 0xdead_beef_cafe_f00du64;
        for _ in 0..20 {
            let (u1, u2) = (xorshift(&mut st) % N, xorshift(&mut st) % N);
            let expect = padd(pmul(u1, Some((GX, GY))), pmul(u2, Some((QX, QY))));
            let mut bad = 0u64;
            let rx = walk(u1, u2, &|i| table[i], |d| modinv(d, P), &mut bad);
            assert_eq!(bad, 0, "honest walk tripped a check");
            assert_eq!(Some(rx), expect.map(|(x, _)| x));
        }
    }

    #[test]
    fn sign_verify_roundtrip() {
        let table = tables((QX, QY));
        for msg in [
            b"hi".as_slice(),
            b"The quick brown fox jumps over the lazy dog",
            &[0x55; MSG_CAP],
            &[0u8; 1],
        ] {
            let (r, s) = sign(D_DEMO, msg);
            assert!(native_verify((QX, QY), msg, r, s));
            let adv = advice(&table, msg, r, s);
            assert_eq!(verify_slices(msg, &table, &adv), 1, "msg len {}", msg.len());
        }
    }

    #[test]
    fn rejects_corruption() {
        let table = tables((QX, QY));
        let msg = b"signed once, tampered later";
        let (r, s) = sign(D_DEMO, msg);
        // Tampered signature halves (advice regenerated honestly for the
        // tampered values — the wrongness must survive its own best effort).
        for (r2, s2) in [(r ^ 2, s), (r, s ^ 2), (r ^ 2, s ^ 2), (s, r)] {
            let adv = advice(&table, msg, r2, s2);
            assert!(!native_verify((QX, QY), msg, r2, s2));
            assert_eq!(verify_slices(msg, &table, &adv), 0);
        }
        // A different message under the same signature.
        let adv = advice(&table, b"some other message", r, s);
        assert_eq!(verify_slices(b"some other message", &table, &adv), 0);
        // Out-of-range and zero signature words.
        for (r2, s2) in [(0, s), (r, 0), (N, s), (r, N), (u64::MAX, u64::MAX)] {
            let adv = advice(&table, msg, r2, s2);
            assert_eq!(verify_slices(msg, &table, &adv), 0);
        }
    }

    /// Soundness probe: every single advice word is load-bearing — flipping
    /// any one of them must flip the verdict to 0 (a cheater can't smuggle
    /// a wrong inverse or scalar past the in-VM checks).
    #[test]
    fn every_advice_word_matters() {
        let table = tables((QX, QY));
        let msg = b"all twenty-eight words audited";
        let (r, s) = sign(D_DEMO, msg);
        let adv = advice(&table, msg, r, s);
        assert_eq!(verify_slices(msg, &table, &adv), 1);
        for k in 0..ADVICE_WORDS {
            for bit in [0, 17, 63] {
                let mut bent = adv;
                bent[k] ^= 1 << bit;
                assert_eq!(verify_slices(msg, &table, &bent), 0, "advice word {k} bit {bit}");
            }
        }
        let zeros = [0u64; ADVICE_WORDS];
        assert_eq!(verify_slices(msg, &table, &zeros), 0);
    }

    /// The branch-free verifier and the textbook one agree on random
    /// (mostly invalid) signatures, not just honest ones.
    #[test]
    fn agrees_with_native_on_random_signatures() {
        let table = tables((QX, QY));
        let mut st = 0x0123_4567_89ab_cdefu64;
        let msg = b"differential";
        for _ in 0..50 {
            let (r, s) = (xorshift(&mut st), xorshift(&mut st));
            let adv = advice(&table, msg, r, s);
            let native = native_verify((QX, QY), msg, r, s);
            assert_eq!(verify_slices(msg, &table, &adv) == 1, native);
        }
    }
}
