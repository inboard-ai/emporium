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
    pub fn columnar(data: serde_json::Value, schema: crate::Schema, metadata: Option<serde_json::Value>) -> Self {
        ToolResult::DataFrame(DataFrame::new(schema, data, metadata))
    }

    /// Get the label from the result, if any
    pub fn label(&self) -> Option<&Label> {
        match self {
            ToolResult::Text(text) => text.label.as_ref(),
            ToolResult::DataFrame(df) => df.label.as_ref(),
        }
    }

    /// Set a label on the result (chainable)
    pub fn with_label(self, label: Label) -> Self {
        match self {
            ToolResult::Text(mut text) => {
                text.label = Some(label);
                ToolResult::Text(text)
            }
            ToolResult::DataFrame(mut df) => {
                df.label = Some(label);
                ToolResult::DataFrame(df)
            }
        }
    }

    /// Get the source description from the result, if any
    pub fn source(&self) -> Option<&str> {
        match self {
            ToolResult::Text(text) => text.source.as_deref(),
            ToolResult::DataFrame(df) => df.source.as_deref(),
        }
    }

    /// Set a human-readable source description (chainable)
    pub fn with_source(self, source: impl Into<String>) -> Self {
        let source = source.into();
        match self {
            ToolResult::Text(mut text) => {
                text.source = Some(source);
                ToolResult::Text(text)
            }
            ToolResult::DataFrame(mut df) => {
                df.source = Some(source);
                ToolResult::DataFrame(df)
            }
        }
    }

    /// Get the per-result store override, if any
    pub fn store(&self) -> Option<bool> {
        match self {
            ToolResult::Text(text) => text.store,
            ToolResult::DataFrame(df) => df.store,
        }
    }

    /// Set a per-result store override (chainable)
    pub fn with_store(self, store: bool) -> Self {
        match self {
            ToolResult::Text(mut text) => {
                text.store = Some(store);
                ToolResult::Text(text)
            }
            ToolResult::DataFrame(mut df) => {
                df.store = Some(store);
                ToolResult::DataFrame(df)
            }
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

/// Execute a custom tool command, handling JSON parsing and response wrapping
pub async fn perform<F, Fut, E>(command: String, correlation_id: Option<String>, executor: F) -> crate::Response<E>
where
    F: FnOnce(serde_json::Value) -> Fut,
    Fut: std::future::Future<Output = Result<ToolResult, E>>,
{
    let request = match serde_json::from_str::<serde_json::Value>(&command) {
        Ok(r) => r,
        Err(_) => {
            return crate::Response::Error {
                message: "Invalid JSON in custom command".to_string(),
                correlation_id,
            };
        }
    };

    let name = match request.get("tool").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return crate::Response::Error {
                message: "Missing 'tool' field in custom command".to_string(),
                correlation_id,
            };
        }
    };

    match executor(request).await {
        Ok(result) => crate::Response::ToolResult {
            name,
            result: Ok(result),
            correlation_id,
        },
        Err(e) => crate::Response::ToolResult {
            name,
            result: Err(e),
            correlation_id,
        },
    }
}
