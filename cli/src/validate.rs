use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Manifest structure matching the extension manifest.toml format
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Manifest {
    pub extension: ExtensionSection,
    pub component: ComponentSection,
    pub config: ConfigSection,
    #[serde(default)]
    pub operations: Option<toml::Table>,
    #[serde(default)]
    pub capabilities: Option<toml::Table>,
    #[serde(default)]
    pub resources: Option<toml::Table>,
    #[serde(default)]
    pub dependencies: Option<toml::Table>,
    #[serde(default)]
    pub package: Option<PackageSection>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExtensionSection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub company: Option<String>,
    pub license: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ComponentSection {
    pub entry: String,
    #[serde(default)]
    pub world: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigSection {
    pub schema: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PackageSection {
    pub created_at: String,
    pub checksum_sha256: String,
    pub packager_version: String,
}

/// Validate an extension directory
pub fn validate_extension(path: &Path) -> Result<Manifest> {
    let manifest = validate_manifest(path)?;
    let wasm_path = path.join(&manifest.component.entry);
    validate_wasm(&wasm_path)?;
    Ok(manifest)
}

/// Validate the manifest.toml file
pub fn validate_manifest(path: &Path) -> Result<Manifest> {
    let manifest_path = path.join("manifest.toml");

    if !manifest_path.exists() {
        bail!("manifest.toml not found at {}", manifest_path.display());
    }

    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read manifest at {}", manifest_path.display()))?;

    let manifest: Manifest = toml::from_str(&content)
        .with_context(|| "Failed to parse manifest.toml")?;

    // Validate required fields in [extension]
    validate_extension_section(&manifest.extension)?;

    // Validate [component] section
    if manifest.component.entry.is_empty() {
        bail!("component.entry is required and cannot be empty");
    }

    // Validate [config] section
    if manifest.config.schema.is_empty() {
        bail!("config.schema is required and cannot be empty");
    }

    // Validate schema is valid JSON
    serde_json::from_str::<serde_json::Value>(&manifest.config.schema)
        .with_context(|| "config.schema must be valid JSON")?;

    Ok(manifest)
}

/// Validate the [extension] section
fn validate_extension_section(ext: &ExtensionSection) -> Result<()> {
    // Validate id format: kebab-case, 3-64 characters
    validate_id(&ext.id)?;

    // Validate name is not empty
    if ext.name.trim().is_empty() {
        bail!("extension.name is required and cannot be empty");
    }

    // Validate version is valid SemVer
    Version::parse(&ext.version)
        .map_err(|e| anyhow!("extension.version must be valid SemVer: {}", e))?;

    // Validate description is not empty
    if ext.description.trim().is_empty() {
        bail!("extension.description is required and cannot be empty");
    }

    // Validate author is not empty
    if ext.author.trim().is_empty() {
        bail!("extension.author is required and cannot be empty");
    }

    // Validate license is not empty
    if ext.license.trim().is_empty() {
        bail!("extension.license is required and cannot be empty");
    }

    Ok(())
}

/// Validate extension id format
/// Must be kebab-case, 2-64 characters, lowercase letters, numbers, and hyphens only
fn validate_id(id: &str) -> Result<()> {
    let len = id.len();

    if len < 2 || len > 64 {
        bail!(
            "extension.id must be 2-64 characters, got {} characters",
            len
        );
    }

    // Check kebab-case format: lowercase letters, numbers, hyphens
    // Cannot start or end with hyphen, no consecutive hyphens
    let chars: Vec<char> = id.chars().collect();

    if chars[0] == '-' || chars[len - 1] == '-' {
        bail!("extension.id cannot start or end with a hyphen");
    }

    for (i, c) in chars.iter().enumerate() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && *c != '-' {
            bail!(
                "extension.id must be kebab-case (lowercase letters, numbers, hyphens only), found '{}'",
                c
            );
        }

        // Check for consecutive hyphens
        if *c == '-' && i > 0 && chars[i - 1] == '-' {
            bail!("extension.id cannot contain consecutive hyphens");
        }
    }

    Ok(())
}

/// Validate a WASM file using wasmparser
pub fn validate_wasm(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("WASM file not found at {}", path.display());
    }

    let wasm_bytes = fs::read(path)
        .with_context(|| format!("Failed to read WASM file at {}", path.display()))?;

    wasmparser::validate(&wasm_bytes)
        .with_context(|| format!("Invalid WASM file at {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_id() {
        assert!(validate_id("kv").is_ok()); // 2 chars is minimum
        assert!(validate_id("key-value").is_ok());
        assert!(validate_id("my-extension-123").is_ok());
        assert!(validate_id("ab").is_ok()); // minimum length
    }

    #[test]
    fn test_invalid_id_format() {
        assert!(validate_id("-invalid").is_err()); // starts with hyphen
        assert!(validate_id("invalid-").is_err()); // ends with hyphen
        assert!(validate_id("in--valid").is_err()); // consecutive hyphens
        assert!(validate_id("Invalid").is_err()); // uppercase
        assert!(validate_id("in_valid").is_err()); // underscore
        assert!(validate_id("in valid").is_err()); // space
    }

    #[test]
    fn test_id_length() {
        assert!(validate_id("a").is_err()); // too short (1 char)
        let long_id = "a".repeat(65);
        assert!(validate_id(&long_id).is_err()); // too long
        let max_id = "a".repeat(64);
        assert!(validate_id(&max_id).is_ok()); // exactly 64
    }
}
