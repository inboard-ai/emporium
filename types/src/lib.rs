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
}
