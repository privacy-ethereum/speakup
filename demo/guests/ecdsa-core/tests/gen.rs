//! Provenance of the toy-64 curve parameters in `lib.rs` — run the search
//! with `cargo test -p ecdsa-core --test gen -- --ignored --nocapture`.
//!
//! The j = 0 family (`y² = x³ + b`, secp256k1's) has CM by √−3, so a prime
//! p ≡ 1 (mod 3) fixes the six twist orders in closed form via the
//! representation 4p = L² + 27M² (modified Cornacchia) — no Schoof needed.
//! The search walks primes p = 2^64 − c until a positive-trace order
//! n = p + 1 − t is prime (positive t keeps n < p, making n pseudo-Mersenne
//! like p), then finds the smallest b in that twist class. Candidate orders
//! are verified rigorously, not just by formula: a prime n in the Hasse
//! interval with n·P = O for 8 random points P forces #E = n. The trace-
//! candidate set itself is validated against brute-force point counts over
//! small primes first.

fn mulmod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
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

/// Deterministic Miller-Rabin, valid for all u64.
fn is_prime(n: u64) -> bool {
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
        let mut x = powmod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..s - 1 {
            x = mulmod(x, x, n);
            if x == n - 1 {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

/// Tonelli-Shanks square root mod an odd prime.
fn sqrt_mod(n: u64, p: u64) -> Option<u64> {
    let n = n % p;
    if n == 0 {
        return Some(0);
    }
    if powmod(n, (p - 1) / 2, p) != 1 {
        return None;
    }
    if p % 4 == 3 {
        return Some(powmod(n, (p + 1) / 4, p));
    }
    let s = (p - 1).trailing_zeros();
    let q = (p - 1) >> s;
    let mut z = 2;
    while powmod(z, (p - 1) / 2, p) != p - 1 {
        z += 1;
    }
    let mut m = s;
    let mut c = powmod(z, q, p);
    let mut t = powmod(n, q, p);
    let mut r = powmod(n, (q + 1) / 2, p);
    while t != 1 {
        let mut i = 0;
        let mut tt = t;
        while tt != 1 {
            tt = mulmod(tt, tt, p);
            i += 1;
        }
        let b = powmod(c, 1 << (m - i - 1), p);
        m = i;
        c = mulmod(b, b, p);
        t = mulmod(t, c, p);
        r = mulmod(r, b, p);
    }
    Some(r)
}

fn isqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Modified Cornacchia (Atkin-Morain): solve 4p = L² + 27M².
fn cornacchia27(p: u64) -> Option<(u128, u128)> {
    // x0² ≡ −27 (mod p), with x0 odd so the congruence lifts mod 4p.
    let mut x0 = sqrt_mod(p - 27 % p, p)?;
    if x0 % 2 == 0 {
        x0 = p - x0;
    }
    let four_p = 4u128 * p as u128;
    let mut a = 2u128 * p as u128;
    let mut b = x0 as u128;
    let l = isqrt(four_p);
    while b > l {
        let r = a % b;
        a = b;
        b = r;
    }
    let t = four_p - b * b;
    if t % 27 != 0 {
        return None;
    }
    let m2 = t / 27;
    let m = isqrt(m2);
    if m * m != m2 {
        return None;
    }
    Some((b, m))
}

/// Candidate traces of the six j = 0 twists from 4p = L² + 27M².
fn candidates(p: u64) -> Vec<i128> {
    let (l, m) = cornacchia27(p).expect("p ≡ 1 mod 3 is always representable");
    let (l, m) = (l as i128, m as i128);
    let mut traces: Vec<i128> = vec![l, -l];
    for t in [l + 9 * m, l - 9 * m, 9 * m - l, -(l + 9 * m)] {
        if t % 2 == 0 {
            traces.push(t / 2);
        }
    }
    traces.sort();
    traces.dedup();
    traces
}

// Affine point ops over y² = x³ + b mod p (None = infinity).
type Pt = Option<(u64, u64)>;

fn add(p1: Pt, p2: Pt, b_unused: u64, p: u64) -> Pt {
    let _ = b_unused;
    let (x1, y1) = match p1 {
        None => return p2,
        Some(q) => q,
    };
    let (x2, y2) = match p2 {
        None => return p1,
        Some(q) => q,
    };
    let lam = if x1 == x2 {
        if (y1 as u128 + y2 as u128) % p as u128 == 0 {
            return None;
        }
        let num = mulmod(3, mulmod(x1, x1, p), p);
        mulmod(num, powmod(((2 * y1 as u128) % p as u128) as u64, p - 2, p), p)
    } else {
        let dx = ((x2 as u128 + p as u128 - x1 as u128) % p as u128) as u64;
        let dy = ((y2 as u128 + p as u128 - y1 as u128) % p as u128) as u64;
        mulmod(dy, powmod(dx, p - 2, p), p)
    };
    let x3 = ((mulmod(lam, lam, p) as u128 + 2 * p as u128 - x1 as u128 - x2 as u128)
        % p as u128) as u64;
    let y3 = ((mulmod(lam, ((x1 as u128 + p as u128 - x3 as u128) % p as u128) as u64, p) as u128
        + p as u128
        - y1 as u128)
        % p as u128) as u64;
    Some((x3, y3))
}

fn smul(mut k: u64, mut g: Pt, b: u64, p: u64) -> Pt {
    let mut acc: Pt = None;
    while k > 0 {
        if k & 1 == 1 {
            acc = add(acc, g, b, p);
        }
        g = add(g, g, b, p);
        k >>= 1;
    }
    acc
}

/// Brute-force #E(F_p) for small p.
fn brute_order(b: u64, p: u64) -> u64 {
    let mut count = 1u64; // infinity
    for x in 0..p {
        let rhs = ((mulmod(x, mulmod(x, x, p), p) as u128 + b as u128) % p as u128) as u64;
        if rhs == 0 {
            count += 1;
        } else if powmod(rhs, (p - 1) / 2, p) == 1 {
            count += 2;
        }
    }
    count
}

/// First curve point with x ≥ start (y the smaller root).
fn first_point(start: u64, b: u64, p: u64) -> (u64, u64) {
    let mut x = start;
    loop {
        let rhs = ((mulmod(x, mulmod(x, x, p), p) as u128 + b as u128) % p as u128) as u64;
        if let Some(y) = sqrt_mod(rhs, p) {
            return (x, y.min(p - y));
        }
        x += 1;
    }
}

/// The trace-candidate formula holds against brute-force counts.
#[test]
fn trace_candidates_cover_small_primes() {
    for p in [31u64, 37, 43, 61, 67, 73, 79, 97, 103, 109, 127, 139, 151, 157, 163] {
        if p % 3 != 1 {
            continue;
        }
        let cands = candidates(p);
        for b in 1..p {
            let t = p as i128 + 1 - brute_order(b, p) as i128;
            assert!(cands.contains(&t), "p={p} b={b}: trace {t} not in {cands:?}");
        }
    }
}

/// The full search. Ignored: it regenerates (and prints) the parameters
/// hardcoded in lib.rs; the non-ignored `parameters_are_consistent` test in
/// lib.rs re-verifies them on every run.
#[test]
#[ignore]
fn regenerate_parameters() {
    let mut rng = 0x9E3779B97F4A7C15u64;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut c = 3u64; // c ≡ 0 mod 3 and odd, so p ≡ 1 mod 3 and odd
    loop {
        let p = 0u64.wrapping_sub(c);
        if !is_prime(p) {
            c += 6;
            continue;
        }
        let hasse = 2 * isqrt(p as u128) as i128 + 2;
        for t in candidates(p).into_iter().filter(|t| *t > 0 && *t <= hasse) {
            let n = (p as i128 + 1 - t) as u64;
            if !is_prime(n) {
                continue;
            }
            for b in 1u64..100 {
                let killed = (0..8).all(|_| {
                    let pt = loop {
                        let x = rand() % p;
                        let rhs = ((mulmod(x, mulmod(x, x, p), p) as u128 + b as u128)
                            % p as u128) as u64;
                        if let Some(y) = sqrt_mod(rhs, p) {
                            break Some((x, y));
                        }
                    };
                    smul(n, pt, b, p).is_none()
                });
                if !killed {
                    continue;
                }
                println!("p = 2^64 - {c} = {p:#x}");
                println!("n = 2^64 - {} = {n:#x} (t = {t})", 0u64.wrapping_sub(n));
                println!("b = {b}");
                let g = first_point(1, b, p);
                println!("G  = ({:#x}, {:#x})", g.0, g.1);
                let d = 2_718_281_828_459_045_235u64 % n;
                let q = smul(d, Some(g), b, p).unwrap();
                println!("d  = {d:#x}");
                println!("Q  = ({:#x}, {:#x})", q.0, q.1);
                let t_pt = first_point(0x1000, b, p);
                let dg = first_point(0x2000, b, p);
                let dq = first_point(0x3000, b, p);
                println!("T  = ({:#x}, {:#x})", t_pt.0, t_pt.1);
                println!("Dg = ({:#x}, {:#x})", dg.0, dg.1);
                println!("Dq = ({:#x}, {:#x})", dq.0, dq.1);
                return;
            }
        }
        c += 6;
    }
}
