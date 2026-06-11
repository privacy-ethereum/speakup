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
//! - `sha256`: SHA-256 of a private message (up to 128 KB); only the digest
//!   is revealed.
//! - `regex`: a private string matches a public pattern (oblivious DFA).
//! - `luhn`: a private card number passes the Luhn checksum.
//! - `csv`: one column of a private CSV document, parsed inside the VM,
//!   averages at least a public threshold.
//! - custom: a user-supplied wasm module; any exported function over
//!   i32/i64 scalars, each argument public or private per a (public)
//!   visibility assignment.

mod port_io;
mod port_mux;
mod regex_table;

pub use port_io::{PortIo, port_io};
pub use port_mux::{PortMux, port_mux};
pub use regex_table::build_table;

use futures::future::join;
use mpz_common::{Context, context::test_st_context};
use mpz_core::Block;
use mpz_ot::{chou_orlandi, ferret, kos};
use mpz_vm_core::{Param, ValType, Vm, Write, value::Value};
use mpz_vm_ir::{ExportKind, Module};
use mpz_vm_zk::{Prover, Verifier};
use rand::{Rng, SeedableRng, rngs::StdRng};
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

// The guest programs, compiled from `../guests` by build.rs.
const SQUARE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/square.wasm"));
const AGE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/age.wasm"));
const SHA256_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sha256.wasm"));
const REGEX_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/regex.wasm"));
const LUHN_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/luhn.wasm"));
const CSV_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/csv.wasm"));

/// The prover's RCOT receiver: Ferret over KOS over a Chou-Orlandi base OT.
type ProverSvole = ferret::Receiver<kos::Receiver<chou_orlandi::Sender>>;
/// The verifier's RCOT sender: Ferret over KOS over a Chou-Orlandi base OT.
type VerifierSvole = ferret::Sender<kos::Sender<chou_orlandi::Receiver>>;

fn err(e: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

/// Initializes threading for this wasm instance: starts the web-spawn
/// spawner and builds the rayon global pool with `threads` workers.
///
/// Must be called once per instance before any `prover_*`/`verifier_*` entry
/// point. The heavy parallel sections (the QuickSilver check, OT transpose)
/// block the calling thread while the pool works, so callers must run in a
/// dedicated worker — never on the main browser thread, where `Atomics.wait`
/// throws. Idempotent: later calls (e.g. other tests in the same instance)
/// are no-ops.
#[wasm_bindgen]
pub async fn initialize(threads: usize) -> Result<(), JsValue> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::Relaxed) {
        return Ok(());
    }

    wasm_bindgen_futures::JsFuture::from(web_spawn::start_spawner()).await?;

    rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .spawn_handler(|thread| {
            // The pool lives for the worker's lifetime; drop the join handle.
            let _ = web_spawn::spawn(move || thread.run());
            Ok(())
        })
        .build_global()
        .map_err(|e| JsError::new(&format!("{e:?}")))?;

    Ok(())
}

/// Builds a muxed [`Context`] over a `MessagePort` to the peer: protocol
/// sub-tasks each get their own logical stream over the port.
fn port_ctx(port: MessagePort) -> Result<Context, JsError> {
    Context::new(port_mux(port).map_err(err)?).map_err(err)
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

/// The embedded guest module for `program`, so the page can show facts
/// about the exact bytes this party runs (size, hash).
#[wasm_bindgen]
pub fn guest_wasm(program: &str) -> Result<Vec<u8>, JsError> {
    let wasm = match program {
        "square" => SQUARE_WASM,
        "age" => AGE_WASM,
        "sha256" => SHA256_WASM,
        "regex" => REGEX_WASM,
        "luhn" => LUHN_WASM,
        "csv" => CSV_WASM,
        _ => return Err(JsError::new(&format!("unknown program: {program}"))),
    };
    Ok(wasm.to_vec())
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

fn prover_from(module: Module) -> Result<Prover<ProverSvole>, JsError> {
    Prover::new(module, prover_svole()).map_err(err)
}

fn verifier_from(module: Module) -> Result<Verifier<VerifierSvole>, JsError> {
    Verifier::new(module, verifier_svole()).map_err(err)
}

fn prover_for(wasm: &[u8]) -> Result<(Prover<ProverSvole>, Module), JsError> {
    let module = parse_module(wasm)?;
    Ok((prover_from(module.clone())?, module))
}

fn verifier_for(wasm: &[u8]) -> Result<(Verifier<VerifierSvole>, Module), JsError> {
    let module = parse_module(wasm)?;
    Ok((verifier_from(module.clone())?, module))
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
    let mut ctx = port_ctx(port)?;
    square_prover_inner(&mut prover, &mut ctx, &module, x).await
}

/// Verifier side of the square program over a `MessagePort` to the prover.
#[wasm_bindgen]
pub async fn verifier_square(port: MessagePort) -> Result<i32, JsError> {
    let (mut verifier, module) = verifier_for(SQUARE_WASM)?;
    let mut ctx = port_ctx(port)?;
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
    let mut ctx = port_ctx(port)?;
    age_prover_inner(&mut prover, &mut ctx, &module, &birthdate, today).await
}

/// Verifier side of the age check. Learns only the revealed 0/1 flag.
#[wasm_bindgen]
pub async fn verifier_age(port: MessagePort, today: i32) -> Result<i32, JsError> {
    let (mut verifier, module) = verifier_for(AGE_WASM)?;
    let mut ctx = port_ctx(port)?;
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

/// Demo cap on the message size; the guest itself only needs `len + 32`
/// bytes of buffer, but proving cost grows linearly with the length.
const SHA256_MSG_CAP: usize = 128 * 1024;

fn check_msg_len(len: usize) -> Result<(), JsError> {
    if len == 0 || len > SHA256_MSG_CAP {
        return Err(JsError::new(&format!(
            "message must be 1..={SHA256_MSG_CAP} bytes"
        )));
    }
    Ok(())
}

/// `cabi_realloc(0, 0, 1, msg_len + 32)`: the message plus digest space.
fn realloc_args(msg_len: usize) -> Vec<Param> {
    vec![
        Param::Public(Value::I32(0)),
        Param::Public(Value::I32(0)),
        Param::Public(Value::I32(1)),
        Param::Public(Value::I32((msg_len + 32) as i32)),
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
            let ptr = expect_i32(
                party
                    .call(ctx, realloc, realloc_args(input.len()))
                    .await
                    .map_err(err)?,
            )? as u32;
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
    let mut ctx = port_ctx(port)?;
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
    let mut ctx = port_ctx(port)?;
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

// === regex ===

/// Looks up the guest's two buffers (public DFA table, private input) — both
/// addresses are public constants.
fn regex_ptrs(vm: &mut impl Vm, module: &Module) -> Result<(u32, u32), JsError> {
    let table = expect_i32(vm.call_local(func(module, "table_ptr")?, vec![]).map_err(err)?)?;
    let input = expect_i32(vm.call_local(func(module, "input_ptr")?, vec![]).map_err(err)?)?;
    Ok((table as u32, input as u32))
}

async fn regex_prover_inner(
    prover: &mut Prover<ProverSvole>,
    ctx: &mut Context,
    module: &Module,
    table: &[u8],
    text: &[u8],
) -> Result<i32, JsError> {
    let (table_ptr, input_ptr) = regex_ptrs(prover, module)?;
    prover.write(table_ptr, Write::Public(table)).map_err(err)?;
    prover.write(input_ptr, Write::Private(text)).map_err(err)?;
    let r = prover
        .call(
            ctx,
            func(module, "matches")?,
            vec![Param::Public(Value::I32(text.len() as i32))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

async fn regex_verifier_inner(
    verifier: &mut Verifier<VerifierSvole>,
    ctx: &mut Context,
    module: &Module,
    table: &[u8],
    text_len: usize,
) -> Result<i32, JsError> {
    let (table_ptr, input_ptr) = regex_ptrs(verifier, module)?;
    verifier.write(table_ptr, Write::Public(table)).map_err(err)?;
    verifier.write(input_ptr, Write::Blind(text_len)).map_err(err)?;
    let r = verifier
        .call(
            ctx,
            func(module, "matches")?,
            vec![Param::Public(Value::I32(text_len as i32))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

fn check_regex_inputs(pattern: &str, text_len: usize) -> Result<Vec<u8>, JsError> {
    if text_len == 0 || text_len > regex_dfa_core::INPUT_CAP {
        return Err(JsError::new(&format!(
            "text must be 1..={} bytes",
            regex_dfa_core::INPUT_CAP
        )));
    }
    build_table(pattern).map_err(|e| JsError::new(&e))
}

/// Prover side of the regex match: proves the private `text` fully matches
/// the public `pattern`. Returns the revealed 0/1 flag.
#[wasm_bindgen]
pub async fn prover_regex(port: MessagePort, pattern: String, text: String) -> Result<i32, JsError> {
    let table = check_regex_inputs(&pattern, text.len())?;
    let module = parse_module(REGEX_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    regex_prover_inner(&mut prover, &mut ctx, &module, &table, text.as_bytes()).await
}

/// Verifier side of the regex match. Knows the pattern and the text length;
/// learns only the revealed 0/1 flag.
#[wasm_bindgen]
pub async fn verifier_regex(
    port: MessagePort,
    pattern: String,
    text_len: usize,
) -> Result<i32, JsError> {
    let table = check_regex_inputs(&pattern, text_len)?;
    let module = parse_module(REGEX_WASM)?;
    let mut verifier = verifier_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    regex_verifier_inner(&mut verifier, &mut ctx, &module, &table, text_len).await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn regex_zkvm(pattern: String, text: String) -> Result<i32, JsError> {
    let table = check_regex_inputs(&pattern, text.len())?;
    let module = parse_module(REGEX_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut verifier = verifier_from(module.clone())?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        regex_prover_inner(&mut prover, &mut ctx_p, &module, &table, text.as_bytes()),
        regex_verifier_inner(&mut verifier, &mut ctx_v, &module, &table, text.len()),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}

// === custom (user-supplied wasm module) ===

/// The exported functions of a module, as JSON for the page to build the
/// parameter UI:
/// `[{"name":…,"params":["i32",…],"results":[…],"supported":bool}]`.
/// `supported` means the zk-vm can call it from here: only i32/i64 scalars,
/// at most one result.
#[wasm_bindgen]
pub fn module_exports(bytes: &[u8]) -> Result<String, JsError> {
    let module = parse_module(bytes)?;
    let ty_name = |t: &ValType| match t {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
    };
    let scalar = |t: &ValType| matches!(t, ValType::I32 | ValType::I64);
    let json_tys = |tys: &[ValType]| {
        tys.iter()
            .map(|t| format!("\"{}\"", ty_name(t)))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut out = Vec::new();
    for e in module.exports() {
        let ExportKind::Func(idx) = e.kind else { continue };
        let Some(f) = module.function(idx) else { continue };
        let ty = f.func_type();
        let supported =
            ty.params.iter().all(scalar) && ty.results.iter().all(scalar) && ty.results.len() <= 1;
        let name = e.name.replace('\\', "\\\\").replace('"', "\\\"");
        out.push(format!(
            r#"{{"name":"{name}","params":[{}],"results":[{}],"supported":{supported}}}"#,
            json_tys(&ty.params),
            json_tys(&ty.results),
        ));
    }
    Ok(format!("[{}]", out.join(",")))
}

/// Builds one party's params for a custom call. `vis[i]` is 1 where argument
/// `i` is the prover's private input; `values[i]` is ignored at private
/// positions on the verifier side. The visibility assignment itself is
/// public — both parties receive the same `vis`.
fn custom_params(
    module: &Module,
    func: u32,
    vis: &[u8],
    values: &[i64],
    prover: bool,
) -> Result<Vec<Param>, JsError> {
    let f = module
        .function(func)
        .ok_or_else(|| JsError::new("function not found"))?;
    let tys = &f.func_type().params;
    if tys.len() != vis.len() || tys.len() != values.len() {
        return Err(JsError::new(&format!(
            "function takes {} arguments, got {}",
            tys.len(),
            values.len()
        )));
    }
    tys.iter()
        .zip(vis.iter().zip(values))
        .map(|(ty, (&private, &raw))| {
            let value = match ty {
                ValType::I32 => Value::I32(
                    i32::try_from(raw).map_err(|_| JsError::new("i32 argument out of range"))?,
                ),
                ValType::I64 => Value::I64(raw),
                _ => return Err(JsError::new("float arguments are not supported")),
            };
            Ok(if private == 0 {
                Param::Public(value)
            } else if prover {
                Param::Private(value)
            } else {
                Param::Blind(*ty)
            })
        })
        .collect()
}

/// Renders a revealed result the same way on both sides.
fn fmt_result(r: Option<Value>) -> Result<String, JsError> {
    Ok(match r {
        Some(Value::I32(x)) => x.to_string(),
        Some(Value::I64(x)) => x.to_string(),
        None => "()".into(),
        other => Err(JsError::new(&format!("unexpected result: {other:?}")))?,
    })
}

/// Prover side of a user-supplied module: calls `func_name` with the given
/// arguments, those marked in `vis` staying private. Returns the revealed
/// result as a string.
#[wasm_bindgen]
pub async fn prover_custom(
    port: MessagePort,
    bytes: Vec<u8>,
    func_name: String,
    vis: Vec<u8>,
    values: Vec<i64>,
) -> Result<String, JsError> {
    let module = parse_module(&bytes)?;
    let mut prover = prover_from(module.clone())?;
    let f = func(&module, &func_name)?;
    let params = custom_params(&module, f, &vis, &values, true)?;
    let mut ctx = port_ctx(port)?;
    let r = prover.call(&mut ctx, f, params).await.map_err(err)?;
    fmt_result(r)
}

/// Verifier side of a user-supplied module. Sees the same module, function,
/// visibility assignment, and public arguments — never the private values.
#[wasm_bindgen]
pub async fn verifier_custom(
    port: MessagePort,
    bytes: Vec<u8>,
    func_name: String,
    vis: Vec<u8>,
    values: Vec<i64>,
) -> Result<String, JsError> {
    let module = parse_module(&bytes)?;
    let mut verifier = verifier_from(module.clone())?;
    let f = func(&module, &func_name)?;
    let params = custom_params(&module, f, &vis, &values, false)?;
    let mut ctx = port_ctx(port)?;
    let r = verifier.call(&mut ctx, f, params).await.map_err(err)?;
    fmt_result(r)
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn custom_zkvm(
    bytes: Vec<u8>,
    func_name: String,
    vis: Vec<u8>,
    values: Vec<i64>,
) -> Result<String, JsError> {
    let module = parse_module(&bytes)?;
    let mut prover = prover_from(module.clone())?;
    let mut verifier = verifier_from(module.clone())?;
    let f = func(&module, &func_name)?;
    let params_p = custom_params(&module, f, &vis, &values, true)?;
    let params_v = custom_params(&module, f, &vis, &values, false)?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        async { prover.call(&mut ctx_p, f, params_p).await.map_err(err) },
        async { verifier.call(&mut ctx_v, f, params_v).await.map_err(err) },
    )
    .await;
    let (rp, rv) = (fmt_result(rp?)?, fmt_result(rv?)?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}

// === luhn (card check) ===

/// Length limits shared with the luhn guest (ISO/IEC 7812).
const LUHN_MIN_LEN: usize = 12;
const LUHN_MAX_LEN: usize = 19;

fn check_luhn_len(len: usize) -> Result<(), JsError> {
    if !(LUHN_MIN_LEN..=LUHN_MAX_LEN).contains(&len) {
        return Err(JsError::new(&format!(
            "number must have {LUHN_MIN_LEN}..={LUHN_MAX_LEN} digits, got {len}"
        )));
    }
    Ok(())
}

async fn luhn_prover_inner(
    prover: &mut Prover<ProverSvole>,
    ctx: &mut Context,
    module: &Module,
    digits: &[u8],
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        prover
            .call_local(func(module, "number_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    prover.write(ptr, Write::Private(digits)).map_err(err)?;
    let r = prover
        .call(
            ctx,
            func(module, "check")?,
            vec![Param::Public(Value::I32(digits.len() as i32))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

async fn luhn_verifier_inner(
    verifier: &mut Verifier<VerifierSvole>,
    ctx: &mut Context,
    module: &Module,
    len: usize,
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        verifier
            .call_local(func(module, "number_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    verifier.write(ptr, Write::Blind(len)).map_err(err)?;
    let r = verifier
        .call(
            ctx,
            func(module, "check")?,
            vec![Param::Public(Value::I32(len as i32))],
        )
        .await
        .map_err(err)?;
    expect_i32(r)
}

/// Prover side of the card check: proves the private `number` (digits, any
/// spacing stripped by the caller) passes the Luhn checksum. Returns the
/// revealed 0/1 flag.
#[wasm_bindgen]
pub async fn prover_luhn(port: MessagePort, number: String) -> Result<i32, JsError> {
    check_luhn_len(number.len())?;
    let module = parse_module(LUHN_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    luhn_prover_inner(&mut prover, &mut ctx, &module, number.as_bytes()).await
}

/// Verifier side of the card check. Learns the length and the revealed 0/1
/// flag, nothing else.
#[wasm_bindgen]
pub async fn verifier_luhn(port: MessagePort, len: usize) -> Result<i32, JsError> {
    check_luhn_len(len)?;
    let module = parse_module(LUHN_WASM)?;
    let mut verifier = verifier_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    luhn_verifier_inner(&mut verifier, &mut ctx, &module, len).await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn luhn_zkvm(number: String) -> Result<i32, JsError> {
    check_luhn_len(number.len())?;
    let module = parse_module(LUHN_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut verifier = verifier_from(module.clone())?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        luhn_prover_inner(&mut prover, &mut ctx_p, &module, number.as_bytes()),
        luhn_verifier_inner(&mut verifier, &mut ctx_v, &module, number.len()),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}

// === csv (column average) ===

/// Bounds shared with the csv guest.
const CSV_CAP: usize = 8192;
const CSV_MAX_COL: i32 = 16;
const CSV_MAX_CELL: i32 = 99_999;

fn check_csv_inputs(len: usize, col: i32, threshold: i32) -> Result<(), JsError> {
    if len == 0 || len > CSV_CAP {
        return Err(JsError::new(&format!("CSV must be 1..={CSV_CAP} bytes")));
    }
    if !(0..CSV_MAX_COL).contains(&col) {
        return Err(JsError::new(&format!("column must be 0..{CSV_MAX_COL}")));
    }
    if !(0..=CSV_MAX_CELL).contains(&threshold) {
        return Err(JsError::new(&format!("threshold must be 0..={CSV_MAX_CELL}")));
    }
    Ok(())
}

async fn csv_prover_inner(
    prover: &mut Prover<ProverSvole>,
    ctx: &mut Context,
    module: &Module,
    csv: &[u8],
    col: i32,
    threshold: i32,
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        prover
            .call_local(func(module, "csv_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    prover.write(ptr, Write::Private(csv)).map_err(err)?;
    let params = vec![
        Param::Public(Value::I32(csv.len() as i32)),
        Param::Public(Value::I32(col)),
        Param::Public(Value::I32(threshold)),
    ];
    let r = prover
        .call(ctx, func(module, "mean_at_least")?, params)
        .await
        .map_err(err)?;
    expect_i32(r)
}

async fn csv_verifier_inner(
    verifier: &mut Verifier<VerifierSvole>,
    ctx: &mut Context,
    module: &Module,
    len: usize,
    col: i32,
    threshold: i32,
) -> Result<i32, JsError> {
    let ptr = expect_i32(
        verifier
            .call_local(func(module, "csv_ptr")?, vec![])
            .map_err(err)?,
    )? as u32;
    verifier.write(ptr, Write::Blind(len)).map_err(err)?;
    let params = vec![
        Param::Public(Value::I32(len as i32)),
        Param::Public(Value::I32(col)),
        Param::Public(Value::I32(threshold)),
    ];
    let r = verifier
        .call(ctx, func(module, "mean_at_least")?, params)
        .await
        .map_err(err)?;
    expect_i32(r)
}

/// Prover side of the CSV column average: the whole document is private;
/// the guest parses it inside the VM and proves the mean of the (public)
/// column reaches the (public) threshold. Returns the revealed 0/1 flag.
#[wasm_bindgen]
pub async fn prover_csv(
    port: MessagePort,
    csv: String,
    col: i32,
    threshold: i32,
) -> Result<i32, JsError> {
    check_csv_inputs(csv.len(), col, threshold)?;
    let module = parse_module(CSV_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    csv_prover_inner(&mut prover, &mut ctx, &module, csv.as_bytes(), col, threshold).await
}

/// Verifier side of the CSV column average. Learns the document length, the
/// column, the threshold, and the revealed 0/1 flag — not the contents, the
/// row count, or the sum.
#[wasm_bindgen]
pub async fn verifier_csv(
    port: MessagePort,
    len: usize,
    col: i32,
    threshold: i32,
) -> Result<i32, JsError> {
    check_csv_inputs(len, col, threshold)?;
    let module = parse_module(CSV_WASM)?;
    let mut verifier = verifier_from(module.clone())?;
    let mut ctx = port_ctx(port)?;
    csv_verifier_inner(&mut verifier, &mut ctx, &module, len, col, threshold).await
}

/// Both parties in this instance (tests / reference).
#[wasm_bindgen]
pub async fn csv_zkvm(csv: String, col: i32, threshold: i32) -> Result<i32, JsError> {
    check_csv_inputs(csv.len(), col, threshold)?;
    let module = parse_module(CSV_WASM)?;
    let mut prover = prover_from(module.clone())?;
    let mut verifier = verifier_from(module.clone())?;
    let (mut ctx_p, mut ctx_v) = test_st_context(1024 * 1024);
    let (rp, rv) = join(
        csv_prover_inner(&mut prover, &mut ctx_p, &module, csv.as_bytes(), col, threshold),
        csv_verifier_inner(&mut verifier, &mut ctx_v, &module, csv.len(), col, threshold),
    )
    .await;
    let (rp, rv) = (rp?, rv?);
    if rp != rv {
        return Err(JsError::new("party results differ"));
    }
    Ok(rp)
}
