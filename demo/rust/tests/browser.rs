//! Browser smoke test: the zk-vm Prover/Verifier pair runs end-to-end in
//! headless Chrome. Run with `wasm-pack test --headless --chrome`.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
use zkvm_demo::square_zkvm;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn square_runs_in_browser() {
    // (6 + 1)² = 49, with 6 as the prover's private input.
    let out = square_zkvm(6).await.expect("zk-vm run should succeed");
    assert_eq!(out, 49);
}
