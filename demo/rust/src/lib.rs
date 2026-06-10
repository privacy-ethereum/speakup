//! Browser bindings for the mpz zk-vm demo.
//!
//! Every run uses the **real OT stack** — Chou-Orlandi base OT, KOS
//! extension, Ferret expansion — the same protocol a production deployment
//! would run, not an ideal functionality.
//!
//! Two ways to drive it:
//!
//! - **Per-party entry points** (`prover_*` / `verifier_*`): each takes a
//!   `MessagePort` and runs one party only. The demo app calls these from two
//!   separate web workers — two isolated wasm memories — with the page
//!   relaying the protocol messages between their ports.
//! - **Single-instance entry points** (`*_zkvm`): both parties in this
//!   instance over an in-memory duplex. Kept for tests and as a reference.
//!
//! Programs:
//! - `square`: `(x + 1)²` over a private `x`.
//! - `age`: the prover's `"YYYY-MM-DD"` birth date stays private; only the
//!   0/1 "18 or older as of today" flag is revealed.
//! - `sha256`: SHA-256 of a private message; only the digest is revealed.

mod port_io;

pub use port_io::{PortIo, port_io};

use futures::future::join;
use mpz_common::{Context, context::test_st_context};
use mpz_core::Block;
use mpz_ot::{chou_orlandi, ferret, kos};
use mpz_vm_core::{Param, Vm, Write, value::Value};
use mpz_vm_ir::{ExportKind, Module};
use mpz_vm_zk::{Prover, Verifier};
use rand::{Rng, SeedableRng, rngs::StdRng};
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

// The guest programs, compiled from `../guests` by build.rs.
const SQUARE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/square.wasm"));
const AGE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/age.wasm"));
const SHA256_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sha256.wasm"));

/// The prover's RCOT receiver: Ferret over KOS over a Chou-Orlandi base OT.
type ProverSvole = ferret::Receiver<kos::Receiver<chou_orlandi::Sender>>;
/// The verifier's RCOT sender: Ferret over KOS over a Chou-Orlandi base OT.
type VerifierSvole = ferret::Sender<kos::Sender<chou_orlandi::Receiver>>;

fn err(e: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

/// The prover's half of the RCOT stack. The base-OT roles are swapped
/// relative to the extension: the prover's KOS receiver is bootstrapped by a
/// Chou-Orlandi sender.
fn prover_svole() -> ProverSvole {
    let mut rng = StdRng::from_os_rng();
    ferret::Receiver::new(
        ferret::FerretConfig::default(),
        rng.random(),
        kos::Receiver::new(kos::ReceiverConfig::default(), chou_orlandi::Sender::new()),
    )
}

/// The verifier's half of the RCOT stack. The verifier is the RCOT sender and
/// holds the correlation `delta` (lsb forced to 1, as the zk-vm requires).
fn verifier_svole() -> VerifierSvole {
    let mut rng = StdRng::from_os_rng();
    let mut delta: Block = rng.random();
    delta.set_lsb(true);
    ferret::Sender::new(
        ferret::FerretConfig::default(),
        rng.random(),
        kos::Sender::new(
            kos::SenderConfig::default(),
            delta,
            chou_orlandi::Receiver::new(),
        ),
    )
}

fn parse_module(wasm: &[u8]) -> Result<Module, JsError> {
    Module::parse(wasm).map_err(err)
}

fn func(module: &Module, name: &str) -> Result<u32, JsError> {
    module
        .exports()
        .iter()
        .find_map(|e| match e.kind {
            ExportKind::Func(idx) if e.name == name => Some(idx),
            _ => None,
        })
        .ok_or_else(|| JsError::new(&format!("export not found: {name}")))
}

fn expect_i32(result: Option<Value>) -> Result<i32, JsError> {
    match result {
        Some(Value::I32(out)) => Ok(out),
        other => Err(JsError::new(&format!("unexpected result: {other:?}"))),
    }
}

fn prover_for(wasm: &[u8]) -> Result<(Prover<ProverSvole>, Module), JsError> {
    let module = parse_module(wasm)?;
    let prover = Prover::new(module.clone(), prover_svole()).map_err(err)?;
    Ok((prover, module))
}

fn verifier_for(wasm: &[u8]) -> Result<(Verifier<VerifierSvole>, Module), JsError> {
    let module = parse_module(wasm)?;
    let verifier = Verifier::new(module.clone(), verifier_svole()).map_err(err)?;
    Ok((verifier, module))
}

// === square ===

async fn square_prover_inner(
    prover: &mut Prover<ProverSvole>,
    ctx: &mut Context,
    module: &Module,
    x: i32,
) -> Result<i32, JsError> {
    let compute = func(module, "compute")?;
    let r = prover
        .call(ctx, compute, vec![Param::Private(Value::I32(x))])
        .await
        .map_err(err)?;
    expect_i32(r)
}

async fn square_verifier_inner(
    verifier: &mut Verifier<VerifierSvole>,
    ctx: &mut Context,
    module: &Module,
) -> Result<i32, JsError> {
    let compute = func(module, "compute")?;
    let r = verifier
        .call(ctx, compute, vec![Param::Blind(mpz_vm_core::ValType::I32)])
        .await
        .map_err(err)?;
    expect_i32(r)
}

/// Prover side of the square program over a `MessagePort` to the verifier.
#[wasm_bindgen]
pub async fn prover_square(port: MessagePort, x: i32) -> Result<i32, JsError> {
    let (mut prover, module) = prover_for(SQUARE_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    square_prover_inner(&mut prover, &mut ctx, &module, x).await
}

/// Verifier side of the square program over a `MessagePort` to the prover.
#[wasm_bindgen]
pub async fn verifier_square(port: MessagePort) -> Result<i32, JsError> {
    let (mut verifier, module) = verifier_for(SQUARE_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    square_verifier_inner(&mut verifier, &mut ctx, &module).await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn square_zkvm(x: i32) -> Result<i32, JsError> {
    let (mut prover, module) = prover_for(SQUARE_WASM)?;
    let (mut verifier, _) = verifier_for(SQUARE_WASM)?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        square_prover_inner(&mut prover, &mut ctx_p, &module, x),
        square_verifier_inner(&mut verifier, &mut ctx_v, &module),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}

// === age ===

fn check_birthdate(birthdate: &str) -> Result<(), JsError> {
    let b = birthdate.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(JsError::new("birthdate must be YYYY-MM-DD"));
    }
    Ok(())
}

/// Length of the age guest's birth-date buffer.
const DATE_LEN: usize = 10;

async fn age_prover_inner(
    prover: &mut Prover<ProverSvole>,
    ctx: &mut Context,
    module: &Module,
    birthdate: &str,
    today: i32,
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        prover
            .call_local(func(module, "birthdate_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    prover
        .write(ptr, Write::Private(birthdate.as_bytes()))
        .map_err(err)?;
    let r = prover
        .call(
            ctx,
            func(module, "is_adult")?,
            vec![Param::Public(Value::I32(today))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

async fn age_verifier_inner(
    verifier: &mut Verifier<VerifierSvole>,
    ctx: &mut Context,
    module: &Module,
    today: i32,
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        verifier
            .call_local(func(module, "birthdate_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    verifier.write(ptr, Write::Blind(DATE_LEN)).map_err(err)?;
    let r = verifier
        .call(
            ctx,
            func(module, "is_adult")?,
            vec![Param::Public(Value::I32(today))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

/// Prover side of the age check: proves the private `birthdate` makes the
/// holder 18+ as of `today` (packed `YYYYMMDD`). Returns the revealed flag.
#[wasm_bindgen]
pub async fn prover_age(port: MessagePort, birthdate: String, today: i32) -> Result<i32, JsError> {
    check_birthdate(&birthdate)?;
    let (mut prover, module) = prover_for(AGE_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    age_prover_inner(&mut prover, &mut ctx, &module, &birthdate, today).await
}

/// Verifier side of the age check. Learns only the revealed 0/1 flag.
#[wasm_bindgen]
pub async fn verifier_age(port: MessagePort, today: i32) -> Result<i32, JsError> {
    let (mut verifier, module) = verifier_for(AGE_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    age_verifier_inner(&mut verifier, &mut ctx, &module, today).await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn age_zkvm(birthdate: String, today: i32) -> Result<i32, JsError> {
    check_birthdate(&birthdate)?;
    let (mut prover, module) = prover_for(AGE_WASM)?;
    let (mut verifier, _) = verifier_for(AGE_WASM)?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        age_prover_inner(&mut prover, &mut ctx_p, &module, &birthdate, today),
        age_verifier_inner(&mut verifier, &mut ctx_v, &module, today),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}

// === sha256 ===

/// Message capacity of the sha256 guest: the digest is written 4 KiB past the
/// start of the message buffer.
const SHA256_MSG_CAP: usize = 4096;

fn check_msg_len(len: usize) -> Result<(), JsError> {
    if len == 0 || len > SHA256_MSG_CAP {
        return Err(JsError::new(&format!(
            "message must be 1..={SHA256_MSG_CAP} bytes"
        )));
    }
    Ok(())
}

fn realloc_args() -> Vec<Param> {
    vec![
        Param::Public(Value::I32(0)),
        Param::Public(Value::I32(0)),
        Param::Public(Value::I32(1)),
        Param::Public(Value::I32((SHA256_MSG_CAP + 32) as i32)),
    ]
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One party's run of the sha256 program: allocate the buffer, stage the
/// message (`Private` bytes for the prover, `Blind` length for the verifier),
/// hash, and read back the revealed digest.
macro_rules! sha256_inner {
    ($name:ident, $party:ty, $stage:expr) => {
        async fn $name(
            party: &mut $party,
            ctx: &mut Context,
            module: &Module,
            input: &Sha256Input<'_>,
        ) -> Result<String, JsError> {
            let realloc = func(module, "cabi_realloc")?;
            let ptr = expect_i32(party.call(ctx, realloc, realloc_args()).await.map_err(err)?)?
                as u32;
            #[allow(clippy::redundant_closure_call)]
            ($stage)(party, ptr, input)?;
            let hash = func(module, "hash")?;
            let params = vec![
                Param::Public(Value::I32(ptr as i32)),
                Param::Public(Value::I32(input.len() as i32)),
            ];
            let digest_ptr = expect_i32(party.call(ctx, hash, params).await.map_err(err)?)? as u32;
            let digest = party.read(digest_ptr, 32).map_err(err)?.to_vec();
            Ok(to_hex(&digest))
        }
    };
}

enum Sha256Input<'a> {
    Message(&'a [u8]),
    Length(usize),
}

impl Sha256Input<'_> {
    fn len(&self) -> usize {
        match self {
            Sha256Input::Message(m) => m.len(),
            Sha256Input::Length(n) => *n,
        }
    }
}

sha256_inner!(
    sha256_prover_inner,
    Prover<ProverSvole>,
    |p: &mut Prover<ProverSvole>, ptr: u32, input: &Sha256Input<'_>| {
        let Sha256Input::Message(msg) = input else {
            return Err(JsError::new("prover needs the message"));
        };
        p.write(ptr, Write::Private(msg)).map_err(err)
    }
);

sha256_inner!(
    sha256_verifier_inner,
    Verifier<VerifierSvole>,
    |v: &mut Verifier<VerifierSvole>, ptr: u32, input: &Sha256Input<'_>| {
        v.write(ptr, Write::Blind(input.len())).map_err(err)
    }
);

/// Prover side of sha256: hashes the private `message`; returns the revealed
/// digest as lowercase hex.
#[wasm_bindgen]
pub async fn prover_sha256(port: MessagePort, message: Vec<u8>) -> Result<String, JsError> {
    check_msg_len(message.len())?;
    let (mut prover, module) = prover_for(SHA256_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    sha256_prover_inner(
        &mut prover,
        &mut ctx,
        &module,
        &Sha256Input::Message(&message),
    )
    .await
}

/// Verifier side of sha256. `msg_len` is public (the verifier always learns
/// the length); returns the revealed digest as lowercase hex.
#[wasm_bindgen]
pub async fn verifier_sha256(port: MessagePort, msg_len: usize) -> Result<String, JsError> {
    check_msg_len(msg_len)?;
    let (mut verifier, module) = verifier_for(SHA256_WASM)?;
    let mut ctx = Context::new_single_threaded(port_io(port));
    sha256_verifier_inner(
        &mut verifier,
        &mut ctx,
        &module,
        &Sha256Input::Length(msg_len),
    )
    .await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn sha256_zkvm(message: Vec<u8>) -> Result<String, JsError> {
    check_msg_len(message.len())?;
    let (mut prover, module) = prover_for(SHA256_WASM)?;
    let (mut verifier, _) = verifier_for(SHA256_WASM)?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        sha256_prover_inner(
            &mut prover,
            &mut ctx_p,
            &module,
            &Sha256Input::Message(&message),
        ),
        sha256_verifier_inner(
            &mut verifier,
            &mut ctx_v,
            &module,
            &Sha256Input::Length(message.len()),
        ),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("parties disagree on the digest"));
    }
    Ok(rp)
}
