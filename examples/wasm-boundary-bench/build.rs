//! Builds the databench guest component with cargo-component (mirroring
//! kv-repl's build.rs) and hands its path to the binary via an env var. We load
//! the raw `.wasm` component directly with our own wasmtime bindgen, so there's
//! no `cargo emporium package` step — the bench uses a bench-specific WIT.

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let ext = Path::new(&manifest_dir).join("extension");

    println!("cargo::rerun-if-changed=extension/src/lib.rs");
    println!("cargo::rerun-if-changed=extension/Cargo.toml");
    println!("cargo::rerun-if-changed=extension/wit/databench.wit");
    println!("cargo::rerun-if-changed=extension/diamonds.csv");

    let status = Command::new("cargo")
        .args(["component", "build", "--release"])
        .current_dir(&ext)
        .status()
        .expect("failed to run `cargo component build` — is cargo-component installed?");
    assert!(status.success(), "cargo component build failed");

    let wasm = ext.join("target/wasm32-wasip1/release/databench_extension.wasm");
    println!("cargo::rustc-env=DATABENCH_WASM={}", wasm.display());
}
