//! Tool result types for the Emporium extension framework

pub mod dataframe;
pub mod text;

pub use dataframe::DataFrame;
pub use text::Text;

use serde::{Deserialize, Serialize};

/// Label for tool results, displayed in the UI (e.g., "AAPL" for stock data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label(pub String);

impl Label {
    pub fn new<T: Into<String>>(value: T) -> Self {
        Self(value.into())
    }
}

impl<T: Into<String>> From<T> for Label {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

/// Tool execution result with type safety
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    /// Text-based result
    Text(Text),
    /// Columnar data that can be converted to polars DataFrame on the client
    DataFrame(DataFrame),
}

impl ToolResult {
    /// Create a new text result
    pub fn text<T: Into<String>>(content: T) -> Self {
        ToolResult::Text(Text::new(content))
    }

    /// Create columnar data that can be converted to DataFrame on the client
    pub fn columnar(
        data: serde_json::Value,
        schema: crate::Schema,
        metadata: Option<serde_json::Value>,
    ) -> Self {
        ToolResult::DataFrame(DataFrame::new(schema, data, metadata))
    }

    /// Get the label from the result, if any
    pub fn label(&self) -> Option<&Label> {
        match self {
            ToolResult::Text(text) => text.label.as_ref(),
            ToolResult::DataFrame(df) => df.label.as_ref(),
        }
    }
}

impl From<Text> for ToolResult {
    fn from(text: Text) -> Self {
        ToolResult::Text(text)
    }
}

impl From<DataFrame> for ToolResult {
    fn from(df: DataFrame) -> Self {
        ToolResult::DataFrame(df)
    }
}
