//! Text tool result type

use super::Label;
use serde::{Deserialize, Serialize};

/// Text-based tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Text {
    /// The text content
    pub content: String,
    /// Optional label for display (e.g., "AAPL" for stock data)
    pub label: Option<Label>,
    /// Human-readable description of the invocation (e.g., the SQL query)
    #[serde(default)]
    pub source: Option<String>,
    /// Per-result cache override — when set, overrides `tool::Info.cacheable`.
    #[serde(default)]
    pub store: Option<bool>,
    /// Brief description of what the result contains
    #[serde(default)]
    pub description: Option<String>,
    /// Short, no-spaces identifier for use in formulas (e.g., "combined_ratio")
    #[serde(default)]
    pub nickname: Option<String>,
}

impl Text {
    /// Create a new text result
    pub fn new<T: Into<String>>(content: T) -> Self {
        Self {
            content: content.into(),
            label: None,
            source: None,
            store: None,
            description: None,
            nickname: None,
        }
    }

    /// Create a new text result with a label
    pub fn with_label<T: Into<String>>(content: T, label: Label) -> Self {
        Self {
            content: content.into(),
            label: Some(label),
            source: None,
            store: None,
            description: None,
            nickname: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_new_populates_content_only() {
        let text = Text::new("hello");
        assert_eq!(text.content, "hello");
        assert!(text.label.is_none());
        assert!(text.source.is_none());
        assert!(text.store.is_none());
        assert!(text.description.is_none());
        assert!(text.nickname.is_none());
    }

    #[test]
    fn text_with_label_populates_content_and_label() {
        let text = Text::with_label("hello", Label::new("L"));
        assert_eq!(text.content, "hello");
        assert_eq!(text.label.as_ref().map(|l| l.0.as_str()), Some("L"));
        assert!(text.source.is_none());
        assert!(text.store.is_none());
        assert!(text.description.is_none());
        assert!(text.nickname.is_none());
    }

    #[test]
    fn text_serde_roundtrip() {
        let original = Text::with_label("body", Label::new("L"));
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Text = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.content, original.content);
        assert_eq!(decoded.label.as_ref().map(|l| l.0.as_str()), Some("L"));
        assert_eq!(decoded.source, original.source);
        assert_eq!(decoded.store, original.store);
        assert_eq!(decoded.description, original.description);
        assert_eq!(decoded.nickname, original.nickname);
    }
}
