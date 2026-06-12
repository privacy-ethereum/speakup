//! Builds the guest programs in `../guests` to wasm and exposes the
//! artifacts to `lib.rs` via `OUT_DIR`, so editing a guest and rebuilding
//! the bindings is one step. Also copies the transcript demo's fixture out
//! of the transcript-verify git checkout (nothing third-party is vendored
//! into this repo).

use std::{env, fs, path::PathBuf, process::Command};

const GUESTS: &[(&str, &str)] = &[
    ("square_guest.wasm", "square.wasm"),
    ("age_guest.wasm", "age.wasm"),
    ("sha256_guest.wasm", "sha256.wasm"),
    ("regex_guest.wasm", "regex.wasm"),
    ("luhn_guest.wasm", "luhn.wasm"),
    ("csv_guest.wasm", "csv.wasm"),
    ("transcript_guest.wasm", "transcript.wasm"),
];

/// The transcript demo's captured exchange, from transcript-verify's
/// checked-in fixture corpus (exact wire bytes; see fixtures/README.md in
/// that crate): a POST with a JSON request body (covered opaquely) whose
/// 201 response carries the assigned id in a JSON body.
const FIXTURE: &str = "jsonplaceholder_post";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let guests = manifest_dir.parent().unwrap().join("guests");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out.join("guests-target");

    // Cargo scans directories recursively for rerun-if-changed.
    for sub in [
        "square",
        "age",
        "sha256",
        "regex",
        "regex-core",
        "luhn",
        "csv",
        "transcript",
        "transcript-core",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={}", guests.join(sub).display());
    }

    let status = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .arg("--manifest-path")
        .arg(guests.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        // Don't leak this build's flags/target settings into the guest build.
        // Cargo config discovery is cwd-based, so run from ../guests: the
        // shared-memory rustflags and build-std in rust/.cargo/config.toml
        // must not apply to the guests (the zk-vm rejects atomics).
        .current_dir(&guests)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to spawn cargo for the guest build");
    assert!(status.success(), "guest build failed");

    let release = target_dir.join("wasm32-unknown-unknown/release");
    for (artifact, dest) in GUESTS {
        fs::copy(release.join(artifact), out.join(dest))
            .unwrap_or_else(|e| panic!("copying {artifact}: {e}"));
    }

    // The transcript fixture lives in the transcript-verify dependency's
    // git checkout (next to its sources); locate it via cargo metadata so
    // the bytes are never copied into this repo.
    let fixtures = transcript_verify_fixtures(&manifest_dir);
    for side in ["sent", "recv"] {
        let src = fixtures.join(format!("{FIXTURE}.{side}.bin"));
        fs::copy(&src, out.join(format!("transcript_fixture.{side}.bin")))
            .unwrap_or_else(|e| panic!("copying {}: {e}", src.display()));
    }
}

/// The `fixtures/` directory of the transcript-verify checkout this build
/// resolved, found by scanning `cargo metadata` for the crate's manifest
/// path. (String scan instead of a JSON dependency; the path contains no
/// JSON escapes on the unix-y platforms this demo builds on.)
fn transcript_verify_fixtures(manifest_dir: &PathBuf) -> PathBuf {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(&cargo)
        .args(["metadata", "--format-version", "1", "--frozen"])
        .current_dir(manifest_dir)
        .output()
        .expect("failed to spawn cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("metadata is utf-8");
    const KEY: &str = "\"manifest_path\":\"";
    let mut at = 0;
    while let Some(i) = json[at..].find(KEY) {
        let start = at + i + KEY.len();
        let end = start + json[start..].find('"').expect("unterminated string");
        let path = &json[start..end];
        if path.ends_with("/transcript-verify/Cargo.toml") {
            return PathBuf::from(path).parent().unwrap().join("fixtures");
        }
        at = end;
    }
    panic!("transcript-verify not found in cargo metadata");
}
