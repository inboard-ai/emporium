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
    /// LLM-facing description for system prompts and search indexing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    /// Searchable topic tags.
    #[serde(default)]
    pub topics: Vec<String>,
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
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub cacheable: bool,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub activity: Option<RawActivity>,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Raw activity display hints from [tools.activity] TOML table.
#[derive(Debug, Deserialize)]
pub(crate) struct RawActivity {
    pub present: String,
    pub past: String,
    pub subject_field: String,
}

/// Raw manifest as stored in manifest.toml
#[derive(Debug, Deserialize)]
pub(crate) struct RawManifest {
    pub extension: ExtensionSection,
    pub component: ComponentSection,
    pub config: ConfigSection,
    #[serde(default)]
    pub tools: Option<Vec<RawTool>>,
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
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
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
    /// Consumed by the package loader; kept here so serde accepts the field.
    #[allow(dead_code)]
    pub entry: String,
    #[serde(default)]
    pub world: Option<String>,
    /// Minimum emporium version required.
    #[allow(dead_code)]
    #[serde(default)]
    pub emporium: Option<String>,
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

        let tools = if let Some(raw_tools) = self.tools {
            raw_tools
                .into_iter()
                .map(|t| {
                    let schema = match t.schema {
                        Some(ref s) => serde_json::from_str(s).map_err(|e| {
                            crate::Error::Custom(format!("Invalid tool schema JSON for '{}': {}", t.id, e))
                        })?,
                        None => serde_json::Value::Null,
                    };
                    let examples = t
                        .examples
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            serde_json::from_str(s).map_err(|e| {
                                crate::Error::Custom(format!(
                                    "Invalid example JSON for tool '{}' example {}: {}",
                                    t.id, i, e
                                ))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(ManifestTool {
                        id: t.id,
                        name: t.name,
                        description: t.description,
                        schema,
                        cacheable: t.cacheable,
                        primary: t.primary,
                        activity: t.activity.map(|a| emporium_types::ManifestActivity {
                            present: a.present,
                            past: a.past,
                            subject_field: a.subject_field,
                        }),
                        examples,
                    })
                })
                .collect::<Result<Vec<_>, crate::Error>>()?
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
            overview: self.extension.overview,
            topics: self.extension.topics,
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    /// Parse a manifest TOML string through the same pipeline used at load
    /// time. Keeps test bodies focused on the shape under test.
    fn parse_manifest(toml_str: &str) -> Result<Manifest, Error> {
        let raw: RawManifest = toml::from_str(toml_str).map_err(|e| Error::Custom(format!("toml parse: {e}")))?;
        raw.into_manifest()
    }

    /// Minimal valid manifest. Individual tests append or override the
    /// `[component]` and other sections as needed.
    const BASE: &str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"

[config]
schema = '{}'
"#;

    #[test]
    fn manifest_world_defaults_to_tool_extension_when_absent() {
        let manifest = parse_manifest(BASE).expect("parse minimal manifest");
        assert_eq!(manifest.world(), "tool-extension");
        assert!(manifest.world.is_none());
    }

    #[test]
    fn manifest_world_returns_declared_value_when_present() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"
world = "block-extension"

[config]
schema = '{}'
"#;
        let manifest = parse_manifest(toml_str).expect("parse block-extension manifest");
        assert_eq!(manifest.world(), "block-extension");
        assert_eq!(manifest.world.as_deref(), Some("block-extension"));
    }

    #[test]
    fn manifest_world_accepts_full_extension_value() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"
world = "full-extension"

[config]
schema = '{}'
"#;
        let manifest = parse_manifest(toml_str).expect("parse full-extension manifest");
        assert_eq!(manifest.world(), "full-extension");
    }

    #[test]
    fn manifest_tools_list_populated_from_raw() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"

[config]
schema = '{}'

[[tools]]
id = "get"
name = "Get"
description = "Fetch a value"

[[tools]]
id = "set"
name = "Set"
description = "Store a value"
"#;
        let manifest = parse_manifest(toml_str).expect("parse tools manifest");
        assert_eq!(manifest.tools.len(), 2);
        assert_eq!(manifest.tools[0].id, "get");
        assert_eq!(manifest.tools[0].name, "Get");
        assert_eq!(manifest.tools[0].description, "Fetch a value");
        assert_eq!(manifest.tools[0].schema, serde_json::Value::Null);
        assert!(!manifest.tools[0].cacheable);
        assert!(!manifest.tools[0].primary);
        assert!(manifest.tools[0].activity.is_none());
        assert!(manifest.tools[0].examples.is_empty());
        assert_eq!(manifest.tools[1].id, "set");
        assert_eq!(manifest.tools[1].name, "Set");
        assert_eq!(manifest.tools[1].description, "Store a value");
    }

    #[test]
    fn manifest_tools_list_empty_by_default() {
        let manifest = parse_manifest(BASE).expect("parse minimal manifest");
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn manifest_capabilities_parsed_as_bool_map() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"

[config]
schema = '{}'

[capabilities]
network = true
filesystem = false
"#;
        let manifest = parse_manifest(toml_str).expect("parse capabilities manifest");
        assert_eq!(manifest.capabilities.get("network"), Some(&true));
        assert_eq!(manifest.capabilities.get("filesystem"), Some(&false));
        assert_eq!(manifest.capabilities.len(), 2);
    }

    #[test]
    fn manifest_overview_and_topics_parsed() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"
overview = "LLM-facing description of capabilities"
topics = ["storage", "data"]

[component]
entry = "x.wasm"

[config]
schema = '{}'
"#;
        let manifest = parse_manifest(toml_str).expect("parse manifest with overview");
        assert_eq!(
            manifest.overview.as_deref(),
            Some("LLM-facing description of capabilities")
        );
        assert_eq!(manifest.topics, vec!["storage", "data"]);
    }

    #[test]
    fn manifest_overview_defaults_to_none() {
        let manifest = parse_manifest(BASE).expect("parse minimal manifest");
        assert!(manifest.overview.is_none());
        assert!(manifest.topics.is_empty());
    }

    #[test]
    fn manifest_tool_with_full_metadata() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"

[config]
schema = '{}'

[[tools]]
id = "fetch"
name = "Fetch Data"
description = "Fetches data from an API"
schema = '{"type":"object","properties":{"symbol":{"type":"string"}},"required":["symbol"]}'
cacheable = true
primary = true
examples = ['{"symbol": "AAPL"}', '{"symbol": "MSFT"}']
[tools.activity]
present = "Fetching"
past = "Fetched"
subject_field = "/symbol"
"#;
        let manifest = parse_manifest(toml_str).expect("parse rich tool manifest");
        assert_eq!(manifest.tools.len(), 1);
        let tool = &manifest.tools[0];
        assert_eq!(tool.id, "fetch");
        assert!(tool.cacheable);
        assert!(tool.primary);
        assert_eq!(tool.examples.len(), 2);
        assert_eq!(tool.examples[0]["symbol"], "AAPL");
        let activity = tool.activity.as_ref().expect("activity present");
        assert_eq!(activity.present, "Fetching");
        assert_eq!(activity.subject_field, "/symbol");
    }

    #[test]
    fn manifest_tool_schema_defaults_to_null() {
        let manifest = parse_manifest(&format!(
            "{}\n[[tools]]\nid = \"t\"\nname = \"T\"\ndescription = \"d\"\n",
            BASE
        ))
        .expect("parse tool without schema");
        assert_eq!(manifest.tools[0].schema, serde_json::Value::Null);
        assert!(!manifest.tools[0].cacheable);
        assert!(!manifest.tools[0].primary);
        assert!(manifest.tools[0].examples.is_empty());
    }

    #[test]
    fn manifest_component_emporium_version_parsed() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"
emporium = "0.2.0"

[config]
schema = '{}'
"#;
        let _manifest = parse_manifest(toml_str).expect("parse manifest with emporium version");
        // emporium version is on ComponentSection but not currently exposed on Manifest
        // This test just verifies parsing doesn't fail
    }

    #[test]
    fn manifest_invalid_config_schema_returns_error() {
        let toml_str = r#"
[extension]
id = "test"
name = "Test"
version = "0.1.0"
author = "Test"
license = "MIT"

[component]
entry = "x.wasm"

[config]
schema = "not valid json{"
"#;
        let err = parse_manifest(toml_str).expect_err("invalid schema must fail");
        assert!(
            matches!(err, Error::Custom(ref msg) if msg.contains("Invalid config schema JSON")),
            "expected Custom error with 'Invalid config schema JSON' message, got: {err:?}"
        );
    }
}
