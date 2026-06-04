//! Operation definitions exported by an extension's block-provider.

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn def_serde_roundtrip_preserves_fields() {
        let original = Def {
            name: "add_prefix".to_string(),
            description: "adds a prefix".to_string(),
            schema: json!({"type": "object"}),
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Def = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.description, original.description);
        assert_eq!(decoded.schema, original.schema);
    }
}
