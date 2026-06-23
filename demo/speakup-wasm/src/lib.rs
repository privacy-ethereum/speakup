//! Generic wasm-bindgen bindings for the mpz zk-vm.
//!
//! This crate knows nothing about any particular guest program. It exposes
//! the zk-vm as a small, JS-driven API: build a [`Party`] (prover or
//! verifier) over a `MessagePort`, hand it a wasm module, then stage memory
//! (`write_*`), call exported functions (`call` / `call_local`), and read
//! revealed bytes (`read`) — one step at a time, from JavaScript.
//!
//! Every run uses the **real OT stack** — Chou-Orlandi base OT, KOS
//! extension, Ferret expansion — the same protocol a production deployment
//! would run, not an ideal functionality.
//!
//! - **`Party::prover` / `Party::verifier`**: one party each, over a
//!   `MessagePort` to the peer. The demo app drives two of these from two
//!   separate web workers — two isolated wasm memories — with the page
//!   relaying the protocol messages between their ports.
//! - The interactive step is [`Party::call`] (returns a `Promise`); `write_*`,
//!   `read`, and `call_local` are local and synchronous.

mod port_io;
mod port_mux;

pub use port_io::{PortIo, port_io};
pub use port_mux::{PortMux, port_mux};

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Promise;
use mpz_common::Context;
use mpz_core::Block;
use mpz_ot::{chou_orlandi, ferret, softspoken};
use mpz_vm_core::{Param, ValType, Vm, Write, value::Value};
use mpz_vm_ir::{ExportKind, Module};
use mpz_vm_zk::{Prover, Verifier};
use rand::{Rng, SeedableRng, rngs::StdRng};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use web_sys::MessagePort;

/// The prover's RCOT receiver: Ferret over SoftSpoken over a Chou-Orlandi base OT.
type ProverSvole = ferret::Receiver<softspoken::Receiver<chou_orlandi::Sender>>;
/// The verifier's RCOT sender: Ferret over SoftSpoken over a Chou-Orlandi base OT.
type VerifierSvole = ferret::Sender<softspoken::Sender<chou_orlandi::Receiver>>;

fn err(e: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

fn busy() -> JsError {
    JsError::new("party is busy (a call is in flight) or already closed")
}

/// Rayon pool size, recorded by [`initialize`]; 1 until it runs.
static THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// Gate bits per parallel proving segment ([`Prover::with_segment_cost`]),
/// scaled inversely with the thread count to hold the segments-per-worker
/// ratio constant — too-fine segments pay an O(segments²) boundary-seeding
/// cost. `1_800_000` anchors to mpz's `sha256` bench (~100k bits at 18
/// workers); `None` at 1 worker proves each chunk as a single segment.
fn segment_cost() -> Option<usize> {
    let threads = THREADS.load(std::sync::atomic::Ordering::Relaxed);
    if threads <= 1 {
        return None;
    }
    Some((1_800_000 / threads).max(50_000))
}

/// Initializes threading for this wasm instance: starts the web-spawn
/// spawner and builds the rayon global pool with `threads` workers.
///
/// Must be called once per instance before any [`Party`] runs. The heavy
/// parallel sections (the QuickSilver check, OT transpose) block the calling
/// thread while the pool works, so callers must run in a dedicated worker —
/// never on the main browser thread, where `Atomics.wait` throws. Idempotent.
#[wasm_bindgen]
pub async fn initialize(threads: usize) -> Result<(), JsValue> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    // Record the pool size before the idempotency guard so `segment_cost` sees
    // it even if a later call short-circuits.
    THREADS.store(threads.max(1), Ordering::Relaxed);
    if STARTED.swap(true, Ordering::Relaxed) {
        return Ok(());
    }

    console_error_panic_hook::set_once();
    init_console_tracing();

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

/// Routes the zk-vm's `tracing` events to the browser console. Console-only
/// (no performance-timeline layer) so it runs inside a web worker, where
/// there is no `window`.
fn init_console_tracing() {
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;
    use tracing_web::MakeWebConsoleWriter;

    let fmt_layer = fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(MakeWebConsoleWriter::new())
        .with_filter(LevelFilter::DEBUG);
    // `try_init` (not `init`): a second call — or a `log` logger already
    // installed by another crate — returns Err rather than panicking the wasm.
    let _ = tracing_subscriber::registry().with(fmt_layer).try_init();
}

/// Builds a muxed [`Context`] over a `MessagePort` to the peer: protocol
/// sub-tasks each get their own logical stream over the port.
fn port_ctx(port: MessagePort) -> Result<Context, JsError> {
    Context::new(port_mux(port).map_err(err)?).map_err(err)
}

/// The prover's half of the RCOT stack. The base-OT roles are swapped
/// relative to the extension: the prover's SoftSpoken receiver is bootstrapped
/// by a Chou-Orlandi sender.
fn prover_svole() -> ProverSvole {
    let mut rng = StdRng::from_os_rng();
    ferret::Receiver::new(
        ferret::FerretConfig::default(),
        rng.random(),
        softspoken::Receiver::new(
            softspoken::ReceiverConfig::default(),
            chou_orlandi::Sender::new(),
        ),
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
        softspoken::Sender::new(
            softspoken::SenderConfig::default(),
            delta,
            chou_orlandi::Receiver::new(),
        ),
    )
}

fn parse_module(wasm: &[u8]) -> Result<Module, JsError> {
    Module::parse(wasm).map_err(err)
}

/// Resolves an exported function name to its index in `module`.
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

fn ty_name(t: &ValType) -> &'static str {
    match t {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
    }
}

fn parse_ty(name: &str) -> Result<ValType, JsError> {
    Ok(match name {
        "i32" => ValType::I32,
        "i64" => ValType::I64,
        "f32" => ValType::F32,
        "f64" => ValType::F64,
        other => return Err(JsError::new(&format!("unknown value type: {other}"))),
    })
}

/// The exported functions of a module, as JSON for the page to inspect:
/// `[{"name":…,"params":["i32",…],"results":[…],"supported":bool}]`.
/// `supported` means the zk-vm can call it: only i32/i64 scalars, at most one
/// result.
fn exports_json(module: &Module) -> String {
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
    format!("[{}]", out.join(","))
}

/// The exported functions of `bytes` as JSON (see [`exports_json`]). Generic:
/// no party or protocol needed, just the module.
#[wasm_bindgen]
pub fn module_exports(bytes: &[u8]) -> Result<String, JsError> {
    Ok(exports_json(&parse_module(bytes)?))
}

// === param / result marshalling ===

fn js_get(obj: &JsValue, key: &str) -> Result<JsValue, JsError> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .map_err(|_| JsError::new(&format!("param is missing field: {key}")))
}

fn js_str(obj: &JsValue, key: &str) -> Result<String, JsError> {
    js_get(obj, key)?
        .as_string()
        .ok_or_else(|| JsError::new(&format!("param field {key} must be a string")))
}

/// Builds a [`Value`] of type `ty` from a JS number. i64 values are passed as
/// JS numbers and so are limited to the exact-integer range (±2^53).
fn make_value(v: &JsValue, ty: ValType) -> Result<Value, JsError> {
    let n = v
        .as_f64()
        .ok_or_else(|| JsError::new("param value must be a number"))?;
    Ok(match ty {
        ValType::I32 => Value::I32(n as i32),
        ValType::I64 => Value::I64(n as i64),
        _ => return Err(JsError::new("only i32/i64 params are supported")),
    })
}

/// Parses a JS array of `{kind:"public"|"private"|"blind", ty:"i32"|"i64",
/// value?}` into [`Param`]s.
fn params_from_js(params: &JsValue) -> Result<Vec<Param>, JsError> {
    let arr = js_sys::Array::from(params);
    let mut out = Vec::with_capacity(arr.length() as usize);
    for p in arr.iter() {
        let ty = parse_ty(&js_str(&p, "ty")?)?;
        let kind = js_str(&p, "kind")?;
        out.push(match kind.as_str() {
            "blind" => Param::Blind(ty),
            "public" => Param::Public(make_value(&js_get(&p, "value")?, ty)?),
            "private" => Param::Private(make_value(&js_get(&p, "value")?, ty)?),
            other => return Err(JsError::new(&format!("unknown param kind: {other}"))),
        });
    }
    Ok(out)
}

/// Renders a call result for JS: i32/floats as numbers, i64 as a BigInt,
/// no result as null.
fn value_to_js(v: Option<Value>) -> JsValue {
    match v {
        Some(Value::I32(x)) => JsValue::from_f64(x as f64),
        Some(Value::I64(x)) => JsValue::from(x), // BigInt
        Some(Value::F32(x)) => JsValue::from_f64(x as f64),
        Some(Value::F64(x)) => JsValue::from_f64(x),
        None => JsValue::NULL,
    }
}

// === the party ===

/// One party's running engine, the variant fixed at construction.
enum Engine {
    Prover(Prover<ProverSvole>),
    Verifier(Verifier<VerifierSvole>),
}

/// All the mutable state of a party, taken out of the cell for the duration
/// of an interactive `call` (so no borrow is held across the await).
struct Inner {
    ctx: Context,
    engine: Engine,
    module: Module,
}

impl Inner {
    fn write(&mut self, ptr: u32, w: Write<'_>) -> Result<(), JsError> {
        match &mut self.engine {
            Engine::Prover(p) => p.write(ptr, w),
            Engine::Verifier(v) => v.write(ptr, w),
        }
        .map_err(err)
    }

    fn read(&self, ptr: u32, len: usize) -> Result<Vec<u8>, JsError> {
        match &self.engine {
            Engine::Prover(p) => p.read(ptr, len),
            Engine::Verifier(v) => v.read(ptr, len),
        }
        .map(<[u8]>::to_vec)
        .map_err(err)
    }

    fn call_local(&mut self, func_idx: u32, params: Vec<Param>) -> Result<Option<Value>, JsError> {
        match &mut self.engine {
            Engine::Prover(p) => p.call_local(func_idx, params),
            Engine::Verifier(v) => v.call_local(func_idx, params),
        }
        .map_err(err)
    }

    async fn call(&mut self, func_idx: u32, params: Vec<Param>) -> Result<Option<Value>, JsError> {
        match &mut self.engine {
            Engine::Prover(p) => p.call(&mut self.ctx, func_idx, params).await,
            Engine::Verifier(v) => v.call(&mut self.ctx, func_idx, params).await,
        }
        .map_err(err)
    }
}

/// A zk-vm party (prover or verifier) the JS orchestration drives directly.
///
/// Held in an `Rc<RefCell<Option<…>>>`: the synchronous methods borrow it
/// briefly; the asynchronous [`Party::call`] takes the state out, runs the
/// protocol round owning it (so the returned `Promise` borrows nothing), then
/// puts it back. Calls must not overlap — `await` each before the next.
#[wasm_bindgen]
pub struct Party {
    inner: Rc<RefCell<Option<Inner>>>,
}

impl Party {
    fn wrap(inner: Inner) -> Party {
        Party {
            inner: Rc::new(RefCell::new(Some(inner))),
        }
    }
}

#[wasm_bindgen]
impl Party {
    /// The prover side, over a `MessagePort` to the verifier, running
    /// `module_wasm`.
    pub fn prover(port: MessagePort, module_wasm: Vec<u8>) -> Result<Party, JsError> {
        let module = parse_module(&module_wasm)?;
        let engine = Engine::Prover(
            Prover::new(module.clone(), prover_svole())
                .map(|p| p.with_segment_cost(segment_cost()))
                .map_err(err)?,
        );
        Ok(Party::wrap(Inner {
            ctx: port_ctx(port)?,
            engine,
            module,
        }))
    }

    /// The verifier side, over a `MessagePort` to the prover, running
    /// `module_wasm`.
    pub fn verifier(port: MessagePort, module_wasm: Vec<u8>) -> Result<Party, JsError> {
        let module = parse_module(&module_wasm)?;
        let engine = Engine::Verifier(
            Verifier::new(module.clone(), verifier_svole())
                .map(|v| v.with_segment_cost(segment_cost()))
                .map_err(err)?,
        );
        Ok(Party::wrap(Inner {
            ctx: port_ctx(port)?,
            engine,
            module,
        }))
    }

    /// The module's exported functions as JSON (see [`module_exports`]).
    pub fn exports(&self) -> Result<String, JsError> {
        Ok(exports_json(&self.inner.borrow().as_ref().ok_or_else(busy)?.module))
    }

    /// Calls a local (non-interactive) export — e.g. a pointer getter or the
    /// `cabi_realloc` allocator. Returns the result value, or null.
    #[wasm_bindgen(js_name = callLocal)]
    pub fn call_local(&self, name: String, params: JsValue) -> Result<JsValue, JsError> {
        let ps = params_from_js(&params)?;
        let mut slot = self.inner.borrow_mut();
        let inner = slot.as_mut().ok_or_else(busy)?;
        let idx = func(&inner.module, &name)?;
        Ok(value_to_js(inner.call_local(idx, ps)?))
    }

    /// Stages this party's private bytes at `ptr` (the prover's `Write::Private`
    /// primitive). The verifier uses [`Party::write_blind`] instead; the
    /// demo's JS API picks the right one by role.
    #[wasm_bindgen(js_name = writePrivate)]
    pub fn write_private(&self, ptr: u32, bytes: Vec<u8>) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .as_mut()
            .ok_or_else(busy)?
            .write(ptr, Write::Private(&bytes))
    }

    /// Stages `bytes` as public input (known to both parties) at `ptr`.
    #[wasm_bindgen(js_name = writePublic)]
    pub fn write_public(&self, ptr: u32, bytes: Vec<u8>) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .as_mut()
            .ok_or_else(busy)?
            .write(ptr, Write::Public(&bytes))
    }

    /// Reserves a blind region of `len` bytes at `ptr` — the peer's private
    /// input, unknown to this party (the verifier's `Write::Blind` primitive).
    #[wasm_bindgen(js_name = writeBlind)]
    pub fn write_blind(&self, ptr: u32, len: usize) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .as_mut()
            .ok_or_else(busy)?
            .write(ptr, Write::Blind(len))
    }

    /// Reads `len` revealed bytes at `ptr`.
    pub fn read(&self, ptr: u32, len: usize) -> Result<Vec<u8>, JsError> {
        self.inner.borrow().as_ref().ok_or_else(busy)?.read(ptr, len)
    }

    /// Calls an interactive export, running one protocol round with the peer.
    /// Resolves to the result value (number / BigInt / null). Both parties
    /// must issue the matching `call` for the round to complete.
    pub fn call(&self, name: String, params: JsValue) -> Promise {
        let inner_rc = self.inner.clone();
        future_to_promise(async move {
            let ps = params_from_js(&params)?;
            // Take the state out so the future owns it (borrows nothing).
            let mut inner = inner_rc.borrow_mut().take().ok_or_else(busy)?;
            let idx = match func(&inner.module, &name) {
                Ok(idx) => idx,
                Err(e) => {
                    inner_rc.borrow_mut().replace(inner);
                    return Err(e.into());
                }
            };
            let res = inner.call(idx, ps).await;
            inner_rc.borrow_mut().replace(inner);
            Ok(value_to_js(res?))
        })
    }
}
