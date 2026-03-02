use crate::validate::validate_manifest;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Build the extension WASM binary using cargo-component.
pub fn build_extension(path: &Path, release: bool) -> Result<()> {
    let manifest = validate_manifest(path)?;
    let entry = &manifest.component.entry;

    println!(
        "Building {} v{} ...",
        manifest.extension.name, manifest.extension.version
    );

    let mut cmd = Command::new("cargo");
    cmd.arg("component").arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(path);

    let status = cmd
        .status()
        .context("Failed to run `cargo component build` — is cargo-component installed?")?;

    if !status.success() {
        bail!("cargo component build failed");
    }

    let wasm_path = path.join(entry);
    if !wasm_path.exists() {
        bail!("Build succeeded but expected WASM not found at {}", wasm_path.display());
    }

    println!("Built: {}", wasm_path.display());
    Ok(())
}
