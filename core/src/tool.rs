//! Tool-related domain types: descriptor metadata and labels.

pub mod data_frame;
pub mod output;
pub mod text;

pub use data_frame::DataFrame;
pub use output::Output;
pub use text::Text;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool metadata exported by an extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    /// Unique identifier for the tool.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of what the tool does.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub schema: Value,
    /// Whether results from this tool should be cached as project-level resources.
    #[serde(default)]
    pub cacheable: bool,
    /// Optional display hints for live/completed status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
    /// Example parameter sets for AI-agent-driven tool discovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Value>,
}

/// Display hints for live/completed status of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    /// Verb shown during execution (e.g. "Querying").
    pub present: String,
    /// Verb shown after completion (e.g. "Queried").
    pub past: String,
    /// JSON pointer into the tool's input params to extract a display subject.
    /// Empty string if no subject.
    pub subject_field: String,
}

/// Label for tool results, displayed in the UI (e.g., "AAPL" for stock data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label(pub String);

impl Label {
    /// Construct a new label from any string-like value.
    pub fn new<T: Into<String>>(value: T) -> Self {
        Self(value.into())
    }
}

impl<T: Into<String>> From<T> for Label {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn info_serde_roundtrip_preserves_all_fields() {
        let original = Info {
            id: "tool-1".to_string(),
            name: "Tool One".to_string(),
            description: "does a thing".to_string(),
            schema: json!({"type": "object"}),
            cacheable: true,
            activity: Some(Activity {
                present: "Querying".to_string(),
                past: "Queried".to_string(),
                subject_field: "/query".to_string(),
            }),
            examples: vec![json!({"key": "test"})],
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Info = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, original.id);
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.description, original.description);
        assert_eq!(decoded.schema, original.schema);
        assert_eq!(decoded.cacheable, original.cacheable);
        assert_eq!(decoded.examples, original.examples);
        let activity = decoded.activity.unwrap();
        assert_eq!(activity.present, "Querying");
        assert_eq!(activity.past, "Queried");
        assert_eq!(activity.subject_field, "/query");
    }

    #[test]
    fn info_with_missing_cacheable_defaults_to_false() {
        let raw = json!({
            "id": "t",
            "name": "T",
            "description": "",
            "schema": {},
        });
        let decoded: Info = serde_json::from_value(raw).unwrap();
        assert!(!decoded.cacheable);
        assert!(decoded.activity.is_none());
        assert!(decoded.examples.is_empty());
    }

    #[test]
    fn info_omits_none_activity_on_serialize() {
        let info = Info {
            id: "t".to_string(),
            name: "T".to_string(),
            description: "".to_string(),
            schema: json!({}),
            cacheable: false,
            activity: None,
            examples: vec![],
        };
        let encoded = serde_json::to_value(&info).unwrap();
        assert!(encoded.get("activity").is_none());
        assert!(encoded.get("examples").is_none());
    }

    #[test]
    fn info_examples_roundtrip() {
        let info = Info {
            id: "t".to_string(),
            name: "T".to_string(),
            description: "".to_string(),
            schema: json!({}),
            cacheable: false,
            activity: None,
            examples: vec![
                json!({"symbol": "AAPL"}),
                json!({"symbol": "MSFT", "outputsize": "full"}),
            ],
        };
        let encoded = serde_json::to_string(&info).unwrap();
        let decoded: Info = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.examples.len(), 2);
        assert_eq!(decoded.examples[0]["symbol"], "AAPL");
    }

    #[test]
    fn activity_serde_roundtrip() {
        let original = Activity {
            present: "Running".to_string(),
            past: "Ran".to_string(),
            subject_field: "/x".to_string(),
        };
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Activity = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.present, original.present);
        assert_eq!(decoded.past, original.past);
        assert_eq!(decoded.subject_field, original.subject_field);
    }

    #[test]
    fn label_from_str_and_string_equivalent() {
        let from_str: Label = "AAPL".into();
        let from_string: Label = String::from("AAPL").into();
        assert_eq!(from_str.0, "AAPL");
        assert_eq!(from_string.0, "AAPL");
    }

    #[test]
    fn label_new_wraps_input() {
        let label = Label::new("MSFT");
        assert_eq!(label.0, "MSFT");
        let label = Label::new(String::from("GOOG"));
        assert_eq!(label.0, "GOOG");
    }
}
