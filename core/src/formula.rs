//! Formula definitions (Phase 2 stub; fleshed out in Phase 4).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Definition of a formula exported by an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Def {
    /// Formula name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema describing the formula's parameters.
    pub schema: Value,
}
