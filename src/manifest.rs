//! Extension manifest types.

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
    pub icon_svg: Option<String>,
    #[serde(default)]
    pub capabilities: HashMap<String, bool>,
    #[serde(default)]
    pub operations: HashMap<String, String>,
    /// JSON Schema for extension configuration/settings
    pub config_schema: serde_json::Value,
    /// WASM component entry point (internal use)
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) component_entry: String,
}

/// Raw manifest as stored in manifest.toml
#[derive(Debug, Deserialize)]
pub(crate) struct RawManifest {
    pub extension: ExtensionSection,
    pub component: ComponentSection,
    pub config: ConfigSection,
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
    pub icon_svg: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ComponentSection {
    pub entry: String,
    #[serde(default)]
    #[allow(dead_code)]
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

        // Convert operations to HashMap<String, String>
        let operations = self.operations.map(|ops| {
            ops.into_iter()
                .filter_map(|(k, v)| {
                    if k == "required" {
                        None // Skip the "required" key
                    } else {
                        Some((k, v.as_str().unwrap_or("").to_string()))
                    }
                })
                .collect()
        }).unwrap_or_default();

        // Convert capabilities to HashMap<String, bool>
        let capabilities = self.capabilities.map(|caps| {
            caps.into_iter()
                .filter_map(|(k, v)| {
                    v.as_bool().map(|b| (k, b))
                })
                .collect()
        }).unwrap_or_default();

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
            icon_svg: self.extension.icon_svg,
            capabilities,
            operations,
            config_schema,
            component_entry: self.component.entry,
        })
    }
}
