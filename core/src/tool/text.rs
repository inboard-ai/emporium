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
    /// Per-result cache override — when set, overrides `ToolInfo.cacheable`
    #[serde(default)]
    pub store: Option<bool>,
}

impl Text {
    /// Create a new text result
    pub fn new<T: Into<String>>(content: T) -> Self {
        Self {
            content: content.into(),
            label: None,
            source: None,
            store: None,
        }
    }

    /// Create a new text result with a label
    pub fn with_label<T: Into<String>>(content: T, label: Label) -> Self {
        Self {
            content: content.into(),
            label: Some(label),
            source: None,
            store: None,
        }
    }
}
