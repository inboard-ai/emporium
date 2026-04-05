//! Column definitions for tabular data.

use serde::{Deserialize, Serialize};

/// Definition of a single column in a tabular tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Def {
    /// Original column name from the data source.
    pub name: String,
    /// Human-friendly display name.
    pub alias: String,
    /// Data type (e.g., "string", "number", "date").
    pub dtype: String,
}
