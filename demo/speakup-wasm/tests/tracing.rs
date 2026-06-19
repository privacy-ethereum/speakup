//! Regression guard for the console-tracing setup. `initialize` installs the
//! `tracing` subscriber, and the zk-vm emits events/spans from the rayon worker
//! threads it spawns. A panicking subscriber init, or a thread-emission path
//! that traps, would crash the wasm — this exercises both. Run with
//! `wasm-pack test --headless --chrome --release -- --features no-bundler`.

#![cfg(target_arch = "wasm32")]

use rayon::prelude::*;
use speakup_wasm::initialize;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

#[wasm_bindgen_test]
async fn tracing_from_worker_threads_does_not_trap() {
    initialize(4).await.expect("threading init");
    let sum: i32 = (0..2000i32)
        .into_par_iter()
        .map(|i| {
            let span = tracing::debug_span!("work", i);
            let _g = span.enter();
            tracing::debug!("inside the span on a rayon thread");
            i
        })
        .sum();
    assert_eq!(sum, 1999000);
}
