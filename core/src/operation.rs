//! Operation definitions (Phase 2 stub; fleshed out in Phase 3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Definition of an operation exported by an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Def {
    /// Operation name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema describing the operation's parameters.
    pub schema: Value,
}
