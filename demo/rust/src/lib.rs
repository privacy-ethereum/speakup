//! Browser bindings for the mpz zk-vm demo.
//!
//! Spike scope: prove the zk-vm stack (`mpz-vm-ir` + `mpz-vm-zk`) runs in
//! browser wasm at all. [`square_zkvm`] runs the full Prover/Verifier pair —
//! both parties in this one wasm instance, joined over an in-memory duplex —
//! executing the `square` guest ((x+1)²) with `x` as the prover's private
//! input, exactly like `crates/vm-zk/tests/square.rs` in mpz.
//!
//! The real demo will split the parties into separate workers with a
//! `MessageChannel` transport; this single-instance version exists to settle
//! the compile-and-run question first.

use futures::future::join;
use mpz_common::context::test_st_context;
use mpz_ot::ideal::rcot::ideal_rcot;
use mpz_vm_core::{Param, Vm, value::Value};
use mpz_vm_ir::{ExportKind, Module};
use mpz_vm_zk::{Prover, Verifier};
use rand::{Rng, SeedableRng, rngs::StdRng};
use wasm_bindgen::prelude::*;

/// The `square` guest from mpz, computing `(x + 1)²` over a private input.
const SQUARE_WASM: &[u8] = include_bytes!("../../../mpz/crates/vm-zk/tests/guests/square.wasm");

fn func_idx(module: &Module, name: &str) -> Result<u32, JsError> {
    module
        .exports()
        .iter()
        .find_map(|e| match e.kind {
            ExportKind::Func(idx) if e.name == name => Some(idx),
            _ => None,
        })
        .ok_or_else(|| JsError::new(&format!("export not found: {name}")))
}

/// Runs the square guest on the zk-vm with `x` as the prover's private input
/// and returns the revealed `(x + 1)²` that both parties learn.
#[wasm_bindgen]
pub async fn square_zkvm(x: i32) -> Result<i32, JsError> {
    let module = Module::parse(SQUARE_WASM).map_err(|e| JsError::new(&format!("{e:?}")))?;
    let idx = func_idx(&module, "compute")?;

    // Ideal RCOT functionality standing in for the OT preprocessing.
    let mut rng = StdRng::seed_from_u64(0);
    let mut delta: mpz_core::Block = rng.random();
    delta.set_lsb(true);
    let (svole_sender, svole_receiver) = ideal_rcot(rng.random(), delta);

    let mut prover =
        Prover::new(module.clone(), svole_receiver).map_err(|e| JsError::new(&format!("{e:?}")))?;
    let mut verifier =
        Verifier::new(module, svole_sender).map_err(|e| JsError::new(&format!("{e:?}")))?;

    let (mut ctx_prover, mut ctx_verifier) = test_st_context(1024 * 1024);

    let prover_params = vec![Param::Private(Value::I32(x))];
    let verifier_params = vec![Param::Blind(mpz_vm_core::ValType::I32)];

    let (result_prover, result_verifier) = join(
        async {
            prover
                .call(&mut ctx_prover, idx, prover_params)
                .await
                .map_err(|e| JsError::new(&format!("prover: {e:?}")))
        },
        async {
            verifier
                .call(&mut ctx_verifier, idx, verifier_params)
                .await
                .map_err(|e| JsError::new(&format!("verifier: {e:?}")))
        },
    )
    .await;

    let (p, v) = (result_prover?, result_verifier?);
    if p != v {
        return Err(JsError::new(&format!("party results differ: {p:?} vs {v:?}")));
    }
    match p {
        Some(Value::I32(out)) => Ok(out),
        other => Err(JsError::new(&format!("unexpected result: {other:?}"))),
    }
}
