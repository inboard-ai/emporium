//! Block type metadata (Phase 2 stub; fleshed out in Phase 3).

use serde::{Deserialize, Serialize};

/// Metadata describing a block type exported by an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    /// Machine-readable kind identifier.
    pub kind: String,
    /// Human-readable display name.
    pub name: String,
    /// Description of what blocks of this type represent.
    pub description: String,
}
