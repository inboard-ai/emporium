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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_preserves_fields() {
        let original = Def {
            name: "foo".to_string(),
            alias: "Foo".to_string(),
            dtype: "string".to_string(),
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Def = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.alias, original.alias);
        assert_eq!(decoded.dtype, original.dtype);
    }
}
