//! Shared types for the Emporium extension ecosystem.
//!
//! Lightweight crate (serde-only deps) for types shared between
//! emporium and shopkeep without pulling in polars.

use serde::{Deserialize, Serialize};

/// Static tool declaration for manifests and registries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    #[serde(default)]
    pub cacheable: bool,
    #[serde(default)]
    pub primary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<ManifestActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<serde_json::Value>,
}

/// Display hints for live/completed status of a tool in manifests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestActivity {
    pub present: String,
    pub past: String,
    pub subject_field: String,
}
