//! Build script for kv-repl that compiles and packages the KV extension.

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let extension_dir = Path::new(&manifest_dir).join("extension");

    // Rerun if extension source changes
    println!("cargo::rerun-if-changed=extension/src/lib.rs");
    println!("cargo::rerun-if-changed=extension/Cargo.toml");
    println!("cargo::rerun-if-changed=extension/manifest.toml");

    // Build the extension with cargo-component
    let status = Command::new("cargo")
        .args(["component", "build", "--release"])
        .current_dir(&extension_dir)
        .status()
        .expect("Failed to run cargo component build. Is cargo-component installed?");

    if !status.success() {
        panic!("cargo component build failed");
    }

    // Package the extension with cargo-emporium
    let status = Command::new("cargo")
        .args(["emporium", "package"])
        .current_dir(&extension_dir)
        .status()
        .expect("Failed to run cargo emporium package. Is cargo-emporium installed?");

    if !status.success() {
        panic!("cargo emporium package failed");
    }

    // Set the extension path as an environment variable for the binary
    let extension_path = extension_dir.join("kv-0.1.0.empkg");
    println!("cargo::rustc-env=KV_EXTENSION_PATH={}", extension_path.display());
}
