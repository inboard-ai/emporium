use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
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
    pub tools: Option<Vec<toml::Table>>,
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
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
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

/// The set of worlds defined in `wit/extension.wit`.
pub const KNOWN_WORLDS: &[&str] = &["tool-extension", "block-extension", "rich-extension", "full-extension"];

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

    let manifest: Manifest = toml::from_str(&content).with_context(|| "Failed to parse manifest.toml")?;

    // Validate required fields in [extension]
    validate_extension_section(&manifest.extension)?;

    // Validate [component] section
    if manifest.component.entry.is_empty() {
        bail!("component.entry is required and cannot be empty");
    }

    // Validate component.world when declared
    validate_world(&manifest.component.world)?;

    // Validate [config] section
    if manifest.config.schema.is_empty() {
        bail!("config.schema is required and cannot be empty");
    }

    // Validate schema is valid JSON
    serde_json::from_str::<serde_json::Value>(&manifest.config.schema)
        .with_context(|| "config.schema must be valid JSON")?;

    // Validate [[tools]] entries if present
    if let Some(ref tools) = manifest.tools {
        for (i, tool) in tools.iter().enumerate() {
            let id = tool.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = tool.get("description").and_then(|v| v.as_str()).unwrap_or("");

            if id.is_empty() {
                bail!("tools[{}].id is required and cannot be empty", i);
            }
            if name.is_empty() {
                bail!("tools[{}].name is required and cannot be empty", i);
            }
            if description.is_empty() {
                bail!("tools[{}].description is required and cannot be empty", i);
            }

            // schema is required and must be valid JSON
            let schema_str = tool.get("schema").and_then(|v| v.as_str()).unwrap_or("");
            if schema_str.is_empty() {
                bail!(
                    "tools[{}] '{}': schema is required — provide a JSON Schema for the tool's parameters",
                    i,
                    id
                );
            }
            if let Err(e) = serde_json::from_str::<serde_json::Value>(schema_str) {
                bail!("tools[{}] '{}': schema must be valid JSON: {}", i, id, e);
            }

            // examples: validate each example is valid JSON when present
            let examples = tool
                .get("examples")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            for (j, example) in examples.iter().enumerate() {
                let example_str = example.as_str().unwrap_or("");
                if !example_str.is_empty()
                    && let Err(e) = serde_json::from_str::<serde_json::Value>(example_str)
                {
                    bail!("tools[{}] '{}': examples[{}] must be valid JSON: {}", i, id, j, e);
                }
            }
        }
    }

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
    Version::parse(&ext.version).map_err(|e| anyhow!("extension.version must be valid SemVer: {}", e))?;

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

    // overview is required for manifest v2
    if ext.overview.as_ref().is_none_or(|o| o.trim().is_empty()) {
        bail!("extension.overview is required — provide an LLM-facing description of the extension's capabilities");
    }

    if ext.topics.is_empty() {
        eprintln!("warning: extension.topics is empty — add topic tags for better search discovery");
    }

    Ok(())
}

/// Validate extension id format
/// Must be kebab-case, 2-64 characters, lowercase letters, numbers, and hyphens only
fn validate_id(id: &str) -> Result<()> {
    let len = id.len();

    if !(2..=64).contains(&len) {
        bail!("extension.id must be 2-64 characters, got {} characters", len);
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

/// Validate the optional `world` field.
///
/// When present, the value must match one of the worlds declared in
/// `wit/extension.wit`.  When absent the host defaults to
/// `"tool-extension"`, so `None` is accepted without error.
fn validate_world(world: &Option<String>) -> Result<()> {
    if let Some(ref w) = *world
        && !KNOWN_WORLDS.contains(&w.as_str())
    {
        bail!("component.world must be one of {:?}, got \"{}\"", KNOWN_WORLDS, w,);
    }
    Ok(())
}

/// Validate manifest `[[tools]]` against the component's runtime tool list.
///
/// Builds a minimal in-memory package from the manifest + built WASM, loads it
/// through the emporium host (which instantiates the component in wasmtime),
/// calls `list_tools()`, and diffs against the manifest's declared tools. Bails
/// on any drift — a tool declared but not exported, exported but not declared,
/// or whose name disagrees.
///
/// The tool list is produced by guest code at runtime, so there is no static
/// shortcut: the component must actually run, which means `init` runs. Config
/// comes from `config_arg` — a JSON file path or an inline JSON object; when none
/// is given and the manifest's config schema declares required fields, this fails
/// explicitly naming them (rather than loading with an empty config and reporting
/// an opaque init error).
pub fn validate_check_sync(path: &Path, config_arg: Option<&str>) -> Result<()> {
    // Validate manifest + wasm, then read both for an in-memory package.
    let manifest = validate_extension(path)?;
    let manifest_toml = fs::read_to_string(path.join("manifest.toml"))?;
    let wasm_path = path.join(&manifest.component.entry);
    let wasm_bytes = fs::read(&wasm_path).with_context(|| format!("Failed to read WASM at {}", wasm_path.display()))?;
    let config = resolve_config(&manifest, config_arg)?;

    // Declared tools: id -> name, from the manifest [[tools]] tables.
    let declared: BTreeMap<String, String> = manifest
        .tools
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|t| {
            let id = t.get("id").and_then(|v| v.as_str())?.to_string();
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            Some((id, name))
        })
        .collect();

    // Runtime tools: load the component and ask it.
    let exported: BTreeMap<String, String> = load_tool_list(&manifest_toml, &wasm_bytes, config)?
        .into_iter()
        .map(|t| (t.id, t.name))
        .collect();

    // Diff by id, plus name mismatches on shared ids.
    let declared_ids: BTreeSet<&String> = declared.keys().collect();
    let exported_ids: BTreeSet<&String> = exported.keys().collect();

    let only_in_manifest: Vec<&&String> = declared_ids.difference(&exported_ids).collect();
    let only_in_component: Vec<&&String> = exported_ids.difference(&declared_ids).collect();
    let name_mismatches: Vec<(&String, &String, &String)> = declared
        .iter()
        .filter_map(|(id, dname)| {
            exported
                .get(id)
                .filter(|ename| *ename != dname)
                .map(|ename| (id, dname, ename))
        })
        .collect();

    for id in &only_in_manifest {
        eprintln!("drift: manifest declares tool '{id}' but the component does not export it");
    }
    for id in &only_in_component {
        eprintln!("drift: component exports tool '{id}' but the manifest does not declare it");
    }
    for (id, dname, ename) in &name_mismatches {
        eprintln!("drift: tool '{id}' name differs — manifest \"{dname}\", component \"{ename}\"");
    }

    if !only_in_manifest.is_empty() || !only_in_component.is_empty() || !name_mismatches.is_empty() {
        bail!(
            "manifest is out of sync with the component ({} declared, {} exported)",
            declared.len(),
            exported.len()
        );
    }

    println!("In sync: {} tools match the component.", declared.len());
    Ok(())
}

/// Build a minimal `.empkg` (manifest.toml + extension.wasm only) in memory and
/// load it through the host to read the runtime tool list. The host's package
/// reader needs just those two entries, so icons/readme — packaging concerns —
/// are skipped here.
fn load_tool_list(
    manifest_toml: &str,
    wasm_bytes: &[u8],
    config: serde_json::Value,
) -> Result<Vec<emporium_core::tool::Info>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    let gz = GzEncoder::new(Vec::new(), Compression::fast());
    let mut ar = Builder::new(gz);
    append_file(&mut ar, "check-sync/manifest.toml", manifest_toml.as_bytes())?;
    append_file(&mut ar, "check-sync/extension.wasm", wasm_bytes)?;
    let pkg = ar.into_inner()?.finish()?;

    let rt = tokio::runtime::Runtime::new().context("failed to start async runtime")?;
    rt.block_on(async {
        let ext = emporium::Extension::from_bytes(&pkg, config).await.context(
            "failed to initialize the component (if it needs configuration, pass --config \
                 <file.json or inline JSON>)",
        )?;
        ext.list_tools().await.context("list_tools() failed")
    })
}

/// Resolve the config passed to the extension's `init`. `--config` accepts
/// either an inline JSON object (a value whose first non-space char is `{`) or a
/// path to a JSON file. Without it, an empty config is used only when the
/// manifest's config schema requires nothing; if the schema lists `required`
/// fields, fail explicitly and name them rather than load with an empty config.
fn resolve_config(manifest: &Manifest, config_arg: Option<&str>) -> Result<serde_json::Value> {
    if let Some(arg) = config_arg {
        if arg.trim_start().starts_with('{') {
            // Inline JSON object.
            return serde_json::from_str(arg.trim()).context("inline --config must be valid JSON");
        }
        // Otherwise a path to a JSON file.
        let raw = fs::read_to_string(arg).with_context(|| format!("Failed to read config file at {arg}"))?;
        return serde_json::from_str(&raw).with_context(|| format!("config file {arg} must be valid JSON"));
    }

    let schema: serde_json::Value =
        serde_json::from_str(&manifest.config.schema).context("manifest config.schema is not valid JSON")?;
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !required.is_empty() {
        bail!(
            "extension '{}' requires configuration to initialize ({}); provide it with --config <file.json or inline JSON>",
            manifest.extension.id,
            required.join(", ")
        );
    }
    Ok(serde_json::json!({}))
}

/// Append in-memory bytes to a tar archive as a 0644 file.
fn append_file<W: std::io::Write>(ar: &mut tar::Builder<W>, path: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    ar.append_data(&mut header, path, data)?;
    Ok(())
}

/// Validate a WASM file using wasmparser
pub fn validate_wasm(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("WASM file not found at {}", path.display());
    }

    let wasm_bytes = fs::read(path).with_context(|| format!("Failed to read WASM file at {}", path.display()))?;

    wasmparser::validate(&wasm_bytes).with_context(|| format!("Invalid WASM file at {}", path.display()))?;

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

    #[test]
    fn valid_world_values_are_accepted() {
        for world in KNOWN_WORLDS {
            let w = Some(world.to_string());
            assert!(
                validate_world(&w).is_ok(),
                "expected world \"{}\" to be accepted",
                world,
            );
        }
    }

    #[test]
    fn unknown_world_is_rejected() {
        let w = Some("mega-extension".to_string());
        let err = validate_world(&w).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("component.world must be one of"),
            "expected error about known worlds, got: {msg}",
        );
        assert!(
            msg.contains("mega-extension"),
            "expected error to mention the invalid value, got: {msg}",
        );
    }

    #[test]
    fn missing_world_is_accepted() {
        assert!(
            validate_world(&None).is_ok(),
            "None world should be accepted (defaults to tool-extension at runtime)",
        );
    }

    #[test]
    fn validate_extension_section_requires_overview() {
        let ext = ExtensionSection {
            id: "test-ext".to_string(),
            name: "Test".to_string(),
            version: "0.1.0".to_string(),
            description: "A test extension".to_string(),
            author: "Test".to_string(),
            company: None,
            license: "MIT".to_string(),
            overview: None,
            topics: vec![],
        };
        let err = validate_extension_section(&ext).unwrap_err();
        assert!(
            err.to_string().contains("overview"),
            "expected overview error, got: {err}"
        );
    }

    #[test]
    fn validate_extension_section_accepts_valid_overview() {
        let ext = ExtensionSection {
            id: "test-ext".to_string(),
            name: "Test".to_string(),
            version: "0.1.0".to_string(),
            description: "A test extension".to_string(),
            author: "Test".to_string(),
            company: None,
            license: "MIT".to_string(),
            overview: Some("LLM-facing capabilities description".to_string()),
            topics: vec!["storage".to_string()],
        };
        assert!(validate_extension_section(&ext).is_ok());
    }
}
