//! Browser smoke test: drive the generic [`Party`] API end-to-end in headless
//! Chrome over the real OT stack, reproducing a SHA-256 digest. The library is
//! guest-agnostic; the guest wasm is embedded here (built by the demo's
//! `build:guest` step into `tests/sha256.wasm`). Run with
//! `wasm-pack test --headless --chrome --release -- --features no-bundler`.

#![cfg(target_arch = "wasm32")]

use speakup_wasm::{Party, initialize};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::MessageChannel;

// The sha256 guest, built by the demo's guest build into this directory.
const GUEST_WASM: &[u8] = include_bytes!("sha256.wasm");

// A dedicated worker, not the page: rayon's parallel sections block the
// calling thread (`Atomics.wait`), which the main browser thread forbids.
wasm_bindgen_test_configure!(run_in_dedicated_worker);

/// Starts web-spawn + the rayon pool (idempotent across tests).
async fn init_threads() {
    initialize(4).await.expect("threading init");
}

/// A param object `{kind, ty:"i32", value?}`.
fn param(kind: &str, value: Option<i32>) -> JsValue {
    let o = js_sys::Object::new();
    js_sys::Reflect::set(&o, &"kind".into(), &kind.into()).unwrap();
    js_sys::Reflect::set(&o, &"ty".into(), &"i32".into()).unwrap();
    if let Some(v) = value {
        js_sys::Reflect::set(&o, &"value".into(), &JsValue::from_f64(v as f64)).unwrap();
    }
    o.into()
}

fn pub32(v: i32) -> JsValue {
    param("public", Some(v))
}

fn params(items: Vec<JsValue>) -> JsValue {
    let arr = js_sys::Array::new();
    for it in items {
        arr.push(&it);
    }
    arr.into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The sha256 orchestration, the same steps the demo's JS runs, but here in
/// Rust against the public API: allocate, stage the message (private for the
/// prover, blind for the verifier), hash, read back the digest.
async fn sha256_party(party: Party, is_prover: bool, message: &[u8]) -> String {
    let len = message.len();
    let ptr = party
        .call_local(
            "cabi_realloc".into(),
            params(vec![pub32(0), pub32(0), pub32(1), pub32((len + 32) as i32)]),
        )
        .unwrap()
        .as_f64()
        .unwrap() as u32;
    if is_prover {
        party.write_private(ptr, message.to_vec()).unwrap();
    } else {
        party.write_blind(ptr, len).unwrap();
    }
    let digest_ptr =
        JsFuture::from(party.call("hash".into(), params(vec![pub32(ptr as i32), pub32(len as i32)])))
            .await
            .unwrap()
            .as_f64()
            .unwrap() as u32;
    hex(&party.read(digest_ptr, 32).unwrap())
}

#[wasm_bindgen_test]
async fn sha256_via_generic_api() {
    init_threads().await;
    let chan = MessageChannel::new().unwrap();
    let prover = Party::prover(chan.port1(), GUEST_WASM.to_vec()).unwrap();
    let verifier = Party::verifier(chan.port2(), GUEST_WASM.to_vec()).unwrap();
    // SHA-256("abc") is the classic test vector.
    let (p, v) = futures::future::join(
        sha256_party(prover, true, b"abc"),
        sha256_party(verifier, false, b"abc"),
    )
    .await;
    assert_eq!(p, v, "parties disagree on the digest");
    assert_eq!(
        p,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[wasm_bindgen_test]
async fn private_scalar_call_in_browser() {
    init_threads().await;
    // inc(x) = x + 1, called with x as a PRIVATE argument (Param::Private on
    // the prover, Param::Blind on the verifier) — the scalar-private call path
    // the demo's argument panel relies on, distinct from the in-memory
    // private bytes the sha256 test exercises.
    let wasm = wat::parse_str(
        r#"(module (func (export "inc") (param i32) (result i32)
            local.get 0 i32.const 1 i32.add))"#,
    )
    .unwrap();
    let chan = MessageChannel::new().unwrap();
    let prover = Party::prover(chan.port1(), wasm.clone()).unwrap();
    let verifier = Party::verifier(chan.port2(), wasm.clone()).unwrap();
    let (p, v) = futures::future::join(
        JsFuture::from(prover.call("inc".into(), params(vec![param("private", Some(41))]))),
        JsFuture::from(verifier.call("inc".into(), params(vec![param("blind", None)]))),
    )
    .await;
    assert_eq!(p.unwrap().as_f64().unwrap() as i32, 42);
    assert_eq!(v.unwrap().as_f64().unwrap() as i32, 42);
}
