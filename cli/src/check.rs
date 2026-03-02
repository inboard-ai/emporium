use crate::validate::validate_manifest;
use anyhow::{Result, bail};
use std::path::Path;

/// Check whether the current extension version is already published on the registry.
/// Exits with code 0 if the version is available, or code 1 if it already exists.
pub fn check_registry(path: &Path, registry: &str) -> Result<()> {
    let manifest = validate_manifest(path)?;
    let id = &manifest.extension.id;
    let version = &manifest.extension.version;

    let url = format!(
        "{}/api/v1/extensions/{}/versions/{}",
        registry.trim_end_matches('/'),
        id,
        version,
    );

    match ureq::get(&url).call() {
        Ok(_) => {
            // 200 — version already exists
            bail!("Version {version} of '{id}' is already published");
        }
        Err(ureq::Error::Status(404, _)) => {
            println!("Version {version} of '{id}' is available for publishing");
            Ok(())
        }
        Err(ureq::Error::Status(status, _)) => {
            bail!("Unexpected response from registry: HTTP {status}");
        }
        Err(e) => {
            bail!("Failed to reach the registry: {e}");
        }
    }
}
