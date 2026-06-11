//! Builds the guest programs in `../guests` to wasm and exposes the
//! artifacts to `lib.rs` via `OUT_DIR`, so editing a guest and rebuilding
//! the bindings is one step.

use std::{env, fs, path::PathBuf, process::Command};

const GUESTS: &[(&str, &str)] = &[
    ("square_guest.wasm", "square.wasm"),
    ("age_guest.wasm", "age.wasm"),
    ("sha256_guest.wasm", "sha256.wasm"),
    ("regex_guest.wasm", "regex.wasm"),
    ("luhn_guest.wasm", "luhn.wasm"),
    ("mean_guest.wasm", "mean.wasm"),
];

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
        "mean",
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
}
