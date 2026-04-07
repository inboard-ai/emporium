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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn def_serde_roundtrip_preserves_fields() {
        let original = Def {
            name: "kv_count".to_string(),
            description: "counts keys".to_string(),
            schema: json!({"type": "array"}),
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Def = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.description, original.description);
        assert_eq!(decoded.schema, original.schema);
    }
}
