//! Extension manifest types.

use emporium_types::ManifestTool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Extension manifest containing all metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub company: Option<String>,
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub capabilities: HashMap<String, bool>,
    #[serde(default)]
    pub tools: Vec<ManifestTool>,
    /// JSON Schema for extension configuration/settings
    pub config_schema: serde_json::Value,
    /// Declared world the extension was built against, e.g. `"tool-extension"`.
    /// When absent the host defaults to `"tool-extension"`.
    #[serde(default)]
    pub world: Option<String>,
    /// WASM component entry point (internal use)
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) component_entry: String,
}

impl Manifest {
    /// Returns the declared world name for this extension. Defaults to
    /// `"tool-extension"` when the manifest does not declare one explicitly.
    pub fn world(&self) -> &str {
        self.world.as_deref().unwrap_or("tool-extension")
    }
}

/// Raw tool entry from [[tools]] TOML array-of-tables.
#[derive(Debug, Deserialize)]
pub(crate) struct RawTool {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// Raw manifest as stored in manifest.toml
#[derive(Debug, Deserialize)]
pub(crate) struct RawManifest {
    pub extension: ExtensionSection,
    pub component: ComponentSection,
    pub config: ConfigSection,
    #[serde(default)]
    pub tools: Option<Vec<RawTool>>,
    /// Kept for backward compatibility during migration.
    #[serde(default)]
    pub operations: Option<HashMap<String, toml::Value>>,
    #[serde(default)]
    pub capabilities: Option<HashMap<String, toml::Value>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExtensionSection {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub company: Option<String>,
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ComponentSection {
    pub entry: String,
    #[serde(default)]
    pub world: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfigSection {
    pub schema: String,
}

impl RawManifest {
    /// Convert raw manifest to public Manifest type
    pub fn into_manifest(self) -> Result<Manifest, crate::Error> {
        let config_schema: serde_json::Value = serde_json::from_str(&self.config.schema)
            .map_err(|e| crate::Error::Custom(format!("Invalid config schema JSON: {}", e)))?;

        // Parse [[tools]] entries, falling back to old [operations] for backward compat
        let tools = if let Some(raw_tools) = self.tools {
            raw_tools
                .into_iter()
                .map(|t| ManifestTool {
                    id: t.id,
                    name: t.name,
                    description: t.description,
                })
                .collect()
        } else if let Some(ops) = &self.operations {
            ops.iter()
                .filter(|(k, _)| k.as_str() != "required")
                .map(|(k, v)| ManifestTool {
                    id: k.clone(),
                    name: k.clone(),
                    description: v.as_str().unwrap_or("").to_string(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // Convert capabilities to HashMap<String, bool>
        let capabilities = self
            .capabilities
            .map(|caps| {
                caps.into_iter()
                    .filter_map(|(k, v)| v.as_bool().map(|b| (k, b)))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Manifest {
            id: self.extension.id,
            name: self.extension.name,
            version: self.extension.version,
            description: self.extension.description,
            author: self.extension.author,
            company: self.extension.company,
            license: self.extension.license,
            homepage: self.extension.homepage,
            repository: self.extension.repository,
            keywords: self.extension.keywords,
            categories: self.extension.categories,
            features: self.extension.features,
            capabilities,
            tools,
            config_schema,
            world: self.component.world,
            component_entry: self.component.entry,
        })
    }
}
