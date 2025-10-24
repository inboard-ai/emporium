//! Host any number of [`Extension`](crate::Extension)s.
use crate::Error;
use crate::data::Id;
use crate::error::ManifestError;
use futures::TryStreamExt;
use sipper::{Sender, Straw, sipper};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

#[derive(Debug, Clone)]
pub struct Manifest {
    pub id: Id,
    pub name: String,
    pub version: String,
    pub description: String,
    pub provider: String,
    pub schema: serde_json::Value,
    pub component_entry: String,
}

pub type Entry = (PathBuf, Manifest);
pub type Result = std::result::Result<(), Error>;

/// Find extensions in a directory recursively (max 2 levels deep).
pub fn list(extensions_dir: impl AsRef<Path>) -> impl Straw<(), Entry, Error> {
    sipper(move |mut sender| async move {
        scan_directory(&extensions_dir.as_ref(), 0, 2, &mut sender).await?;
        Ok(())
    })
}

/// Recursively scan directories for manifest.toml files
fn scan_directory<'a>(
    dir: &'a Path,
    current_depth: usize,
    max_depth: usize,
    sender: &'a mut Sender<Entry>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
        if current_depth >= max_depth {
            return Ok(());
        }

        let entries = fs::read_dir(dir).await?;
        let mut entries = ReadDirStream::new(entries);

        while let Some(entry) = entries.try_next().await? {
            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectory
                scan_directory(&path, current_depth + 1, max_depth, sender).await?;
            } else if path.file_name().and_then(|n| n.to_str()) == Some("manifest.toml") {
                // Found a manifest file - try to parse it
                if let Ok(manifest) = parse_manifest(&path).await {
                    // Verify the wasm file exists
                    let extension_dir = path.parent().unwrap();
                    let wasm_path = extension_dir.join(&manifest.component_entry);

                    if wasm_path.exists() {
                        sender.send((wasm_path, manifest)).await;
                    }
                }
            }
        }

        Ok(())
    })
}

/// Parse a manifest.toml file
async fn parse_manifest(manifest_path: &Path) -> std::result::Result<Manifest, Error> {
    let content = fs::read_to_string(manifest_path)
        .await
        .map_err(|e| ManifestError::ReadError(format!("Failed to read manifest: {}", e)))?;

    let toml: toml::Value =
        toml::from_str(&content).map_err(|e| ManifestError::ReadError(format!("Failed to parse TOML: {}", e)))?;

    let missing =
        |section: &str, field: &str| Error::from(ManifestError::Missing(section.to_string(), field.to_string()));

    let extension = toml.get("extension").ok_or_else(|| missing("section", "extension"))?;
    let component = toml.get("component").ok_or_else(|| missing("section", "component"))?;
    let config = toml.get("config").ok_or_else(|| missing("section", "config"))?;

    Ok(Manifest {
        id: extension
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| missing("extension", "id"))?
            .to_string(),
        name: extension
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| missing("extension", "name"))?
            .to_string(),
        version: extension
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| missing("extension", "version"))?
            .to_string(),
        description: extension
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        provider: extension
            .get("company")
            .and_then(|v| v.as_str())
            .or_else(|| extension.get("author").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string(),
        component_entry: component
            .get("entry")
            .and_then(|v| v.as_str())
            .ok_or_else(|| missing("component", "entry"))?
            .to_string(),
        schema: config
            .get("schema")
            .and_then(|s| s.as_str())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| serde_json::json!({})),
    })
}
