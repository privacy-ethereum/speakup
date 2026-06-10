//! Browser bindings for the mpz zk-vm demo.
//!
//! Each entry point runs the full Prover/Verifier pair over the **real OT
//! stack** — Chou-Orlandi base OT, KOS extension, Ferret expansion — the same
//! protocol a production deployment would run, not an ideal functionality.
//! Both parties still live in this one wasm instance, joined over an
//! in-memory duplex; splitting them into separate workers over a
//! `MessageChannel` transport is the next milestone (the bindings here are
//! deliberately shaped so only the channel changes).
//!
//! Programs:
//! - [`square_zkvm`]: `(x + 1)²` over a private `x`.
//! - [`age_zkvm`]: the prover's `"YYYY-MM-DD"` birth date stays private; only
//!   the 0/1 "18 or older as of today" flag is revealed.
//! - [`sha256_zkvm`]: SHA-256 of a private message; only the digest is
//!   revealed.

use futures::future::join;
use mpz_common::{Context, context::test_st_context};
use mpz_core::Block;
use mpz_ot::{chou_orlandi, ferret, kos};
use mpz_vm_core::{Param, Vm, Write, value::Value};
use mpz_vm_ir::{ExportKind, Module};
use mpz_vm_zk::{Prover, Verifier};
use rand::{Rng, SeedableRng, rngs::StdRng};
use wasm_bindgen::prelude::*;

const SQUARE_WASM: &[u8] = include_bytes!("../../../mpz/crates/vm-zk/tests/guests/square.wasm");
const AGE_WASM: &[u8] = include_bytes!("../../../mpz/crates/vm-zk/tests/guests/age.wasm");
const SHA256_WASM: &[u8] = include_bytes!("../../../mpz/crates/vm-zk/benches/guests/sha256.wasm");

/// The prover's RCOT receiver: Ferret over KOS over a Chou-Orlandi base OT.
type ProverSvole = ferret::Receiver<kos::Receiver<chou_orlandi::Sender>>;
/// The verifier's RCOT sender: Ferret over KOS over a Chou-Orlandi base OT.
type VerifierSvole = ferret::Sender<kos::Sender<chou_orlandi::Receiver>>;

fn err(e: impl std::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

/// Builds the real RCOT stack for both parties. The verifier is the RCOT
/// sender and holds the correlation `delta` (lsb forced to 1, as the zk-vm
/// requires); the base-OT roles are swapped relative to the extension.
fn rcot_stack() -> (VerifierSvole, ProverSvole) {
    let mut rng = StdRng::from_os_rng();
    let mut delta: Block = rng.random();
    delta.set_lsb(true);

    let verifier = ferret::Sender::new(
        ferret::FerretConfig::default(),
        rng.random(),
        kos::Sender::new(
            kos::SenderConfig::default(),
            delta,
            chou_orlandi::Receiver::new(),
        ),
    );
    let prover = ferret::Receiver::new(
        ferret::FerretConfig::default(),
        rng.random(),
        kos::Receiver::new(kos::ReceiverConfig::default(), chou_orlandi::Sender::new()),
    );
    (verifier, prover)
}

/// A live prover/verifier pair over the real OT stack, joined by an
/// in-memory duplex.
struct Session {
    prover: Prover<ProverSvole>,
    verifier: Verifier<VerifierSvole>,
    ctx_p: Context,
    ctx_v: Context,
    module: Module,
}

impl Session {
    fn new(wasm: &[u8]) -> Result<Self, JsError> {
        let module = Module::parse(wasm).map_err(err)?;
        let (v_svole, p_svole) = rcot_stack();
        let prover = Prover::new(module.clone(), p_svole).map_err(err)?;
        let verifier = Verifier::new(module.clone(), v_svole).map_err(err)?;
        let (ctx_p, ctx_v) = test_st_context(1024 * 1024);
        Ok(Self {
            prover,
            verifier,
            ctx_p,
            ctx_v,
            module,
        })
    }

    fn func(&self, name: &str) -> Result<u32, JsError> {
        self.module
            .exports()
            .iter()
            .find_map(|e| match e.kind {
                ExportKind::Func(idx) if e.name == name => Some(idx),
                _ => None,
            })
            .ok_or_else(|| JsError::new(&format!("export not found: {name}")))
    }

    /// Drives one call on both parties concurrently and returns the agreed
    /// result.
    async fn call_both(
        &mut self,
        func: u32,
        p_params: Vec<Param>,
        v_params: Vec<Param>,
    ) -> Result<Option<Value>, JsError> {
        let (rp, rv) = join(
            self.prover.call(&mut self.ctx_p, func, p_params),
            self.verifier.call(&mut self.ctx_v, func, v_params),
        )
        .await;
        let (rp, rv) = (
            rp.map_err(|e| JsError::new(&format!("prover: {e:?}")))?,
            rv.map_err(|e| JsError::new(&format!("verifier: {e:?}")))?,
        );
        if rp != rv {
            return Err(JsError::new(&format!(
                "party results differ: {rp:?} vs {rv:?}"
            )));
        }
        Ok(rp)
    }

    /// Calls a function that both parties can evaluate locally (it touches
    /// nothing private), asserting agreement.
    fn call_local_both(&mut self, func: u32) -> Result<u32, JsError> {
        let p = self.prover.call_local(func, vec![]).map_err(err)?;
        let v = self.verifier.call_local(func, vec![]).map_err(err)?;
        match (p, v) {
            (Some(Value::I32(a)), Some(Value::I32(b))) if a == b => Ok(a as u32),
            other => Err(JsError::new(&format!("local call mismatch: {other:?}"))),
        }
    }

    fn expect_i32(result: Option<Value>) -> Result<i32, JsError> {
        match result {
            Some(Value::I32(out)) => Ok(out),
            other => Err(JsError::new(&format!("unexpected result: {other:?}"))),
        }
    }
}

/// Runs the square guest with `x` as the prover's private input and returns
/// the revealed `(x + 1)²`.
#[wasm_bindgen]
pub async fn square_zkvm(x: i32) -> Result<i32, JsError> {
    let mut s = Session::new(SQUARE_WASM)?;
    let compute = s.func("compute")?;
    let result = s
        .call_both(
            compute,
            vec![Param::Private(Value::I32(x))],
            vec![Param::Blind(mpz_vm_core::ValType::I32)],
        )
        .await?;
    Session::expect_i32(result)
}

/// Proves the holder of the private `"YYYY-MM-DD"` `birthdate` is 18 or older
/// as of `today` (packed `YYYYMMDD`). Returns the revealed flag: 1 if 18+,
/// else 0. The verifier never sees the date.
#[wasm_bindgen]
pub async fn age_zkvm(birthdate: String, today: i32) -> Result<i32, JsError> {
    let bytes = birthdate.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(JsError::new("birthdate must be YYYY-MM-DD"));
    }

    let mut s = Session::new(AGE_WASM)?;
    let ptr = {
        let f = s.func("birthdate_ptr")?;
        s.call_local_both(f)?
    };
    s.prover.write(ptr, Write::Private(bytes)).map_err(err)?;
    s.verifier.write(ptr, Write::Blind(bytes.len())).map_err(err)?;

    let is_adult = s.func("is_adult")?;
    let params = vec![Param::Public(Value::I32(today))];
    let result = s.call_both(is_adult, params.clone(), params).await?;
    Session::expect_i32(result)
}

/// Message capacity of the sha256 guest: the digest is written 4 KiB past the
/// start of the message buffer.
const SHA256_MSG_CAP: usize = 4096;

/// Hashes the prover's private `message` on the zk-vm and returns the
/// revealed digest as lowercase hex. The verifier never sees the message.
#[wasm_bindgen]
pub async fn sha256_zkvm(message: Vec<u8>) -> Result<String, JsError> {
    if message.is_empty() || message.len() > SHA256_MSG_CAP {
        return Err(JsError::new(&format!(
            "message must be 1..={SHA256_MSG_CAP} bytes"
        )));
    }

    let mut s = Session::new(SHA256_WASM)?;

    // Allocate the message region plus the digest through the guest's
    // allocator, on both sides (public arguments, public result).
    let realloc = s.func("cabi_realloc")?;
    let alloc_args = || {
        vec![
            Param::Public(Value::I32(0)),
            Param::Public(Value::I32(0)),
            Param::Public(Value::I32(1)),
            Param::Public(Value::I32((SHA256_MSG_CAP + 32) as i32)),
        ]
    };
    let ptr = match s.call_both(realloc, alloc_args(), alloc_args()).await? {
        Some(Value::I32(p)) => p as u32,
        other => return Err(JsError::new(&format!("cabi_realloc returned {other:?}"))),
    };

    s.prover.write(ptr, Write::Private(&message)).map_err(err)?;
    s.verifier
        .write(ptr, Write::Blind(message.len()))
        .map_err(err)?;

    // hash(ptr, len) reveals the 32-byte digest and returns its address.
    let hash = s.func("hash")?;
    let params = vec![
        Param::Public(Value::I32(ptr as i32)),
        Param::Public(Value::I32(message.len() as i32)),
    ];
    let digest_ptr = match s.call_both(hash, params.clone(), params).await? {
        Some(Value::I32(p)) => p as u32,
        other => return Err(JsError::new(&format!("hash returned {other:?}"))),
    };

    // Both parties read the now-public digest; they must agree.
    let dp = s.prover.read(digest_ptr, 32).map_err(err)?.to_vec();
    let dv = s.verifier.read(digest_ptr, 32).map_err(err)?.to_vec();
    if dp != dv {
        return Err(JsError::new("parties disagree on the digest"));
    }
    Ok(dp.iter().map(|b| format!("{b:02x}")).collect())
}
