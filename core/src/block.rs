//! Block type metadata exported by an extension's block-provider.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_info_serde_roundtrip_preserves_fields() {
        let original = TypeInfo {
            kind: "prefix-tracker".to_string(),
            name: "Prefix Tracker".to_string(),
            description: "tracks key prefixes".to_string(),
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: TypeInfo = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.kind, original.kind);
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.description, original.description);
    }
}
