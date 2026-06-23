//! Shared types for the Emporium extension ecosystem.
//!
//! Lightweight crate (serde-only deps) for types shared between
//! emporium and shopkeep without pulling in polars.

use serde::{Deserialize, Serialize};

/// Static tool declaration for manifests and registries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ManifestActivity {
    pub present: String,
    pub past: String,
    pub subject_field: String,
}

/// Static data-source declaration for manifests — the cheap catalog mirror so
/// the host can render the data-capability picker without instantiating WASM
/// (the parallel to [`ManifestTool`] for `data-provider`). For a static catalog
/// (`is-catalog-dynamic == false`) this is authoritative; dynamic catalogs are
/// discovered at runtime via `list-sources`. See
/// `docs/ui-data-capability-protocol.md` §7.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ManifestDataSource {
    pub id: String,
    pub display_name: String,
    pub description: String,
    /// How the host gathers fetch inputs: `"params"`, `"browse"`,
    /// `"params-and-browse"`, or `"free-query"`.
    pub input: String,
    /// JSON Schema for the param form — present when `input` is `"params"` or
    /// `"params-and-browse"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_schema: Option<serde_json::Value>,
    /// What the host knows about output shape before fetch: `"known"` or
    /// `"resolved"`.
    pub output: String,
    /// Statically-known output columns — present when `output` is `"known"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ManifestColumn>,
    /// Row-level cardinality: `"rows"` (default), `"record"`, or `"scalar"`.
    #[serde(default)]
    pub cardinality: String,
}

/// A statically-declared output column on a [`ManifestDataSource`]. Mirrors the
/// shared WIT `column-def` (`name`, `alias`, `dtype`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ManifestColumn {
    pub name: String,
    pub alias: String,
    pub dtype: String,
}
