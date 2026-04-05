//! Tool execution output: the payload returned by a tool invocation.

use super::{DataFrame, Label, Text};
use crate::column;
use serde::{Deserialize, Serialize};

/// The result of a successful tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Output {
    /// Text-based output.
    Text(Text),
    /// Columnar data that can be converted to a polars DataFrame on the client.
    DataFrame(DataFrame),
}

impl Output {
    /// Create a new text output.
    pub fn text<T: Into<String>>(content: T) -> Self {
        Output::Text(Text::new(content))
    }

    /// Create a new columnar output.
    pub fn columnar(data: serde_json::Value, schema: Vec<column::Def>, metadata: Option<serde_json::Value>) -> Self {
        Output::DataFrame(DataFrame::new(schema, data, metadata))
    }

    /// Get the label from the output, if any.
    pub fn label(&self) -> Option<&Label> {
        match self {
            Output::Text(text) => text.label.as_ref(),
            Output::DataFrame(df) => df.label.as_ref(),
        }
    }

    /// Set a label on the output (chainable).
    pub fn with_label(self, label: Label) -> Self {
        match self {
            Output::Text(mut text) => {
                text.label = Some(label);
                Output::Text(text)
            }
            Output::DataFrame(mut df) => {
                df.label = Some(label);
                Output::DataFrame(df)
            }
        }
    }

    /// Get the source description from the output, if any.
    pub fn source(&self) -> Option<&str> {
        match self {
            Output::Text(text) => text.source.as_deref(),
            Output::DataFrame(df) => df.source.as_deref(),
        }
    }

    /// Set a human-readable source description (chainable).
    pub fn with_source(self, source: impl Into<String>) -> Self {
        let source = source.into();
        match self {
            Output::Text(mut text) => {
                text.source = Some(source);
                Output::Text(text)
            }
            Output::DataFrame(mut df) => {
                df.source = Some(source);
                Output::DataFrame(df)
            }
        }
    }

    /// Get the per-output store override, if any.
    pub fn store(&self) -> Option<bool> {
        match self {
            Output::Text(text) => text.store,
            Output::DataFrame(df) => df.store,
        }
    }

    /// Set a per-output store override (chainable).
    pub fn with_store(self, store: bool) -> Self {
        match self {
            Output::Text(mut text) => {
                text.store = Some(store);
                Output::Text(text)
            }
            Output::DataFrame(mut df) => {
                df.store = Some(store);
                Output::DataFrame(df)
            }
        }
    }

    /// Get the description from the output, if any.
    pub fn description(&self) -> Option<&str> {
        match self {
            Output::Text(text) => text.description.as_deref(),
            Output::DataFrame(df) => df.description.as_deref(),
        }
    }

    /// Set a brief description (chainable).
    pub fn with_description(self, description: impl Into<String>) -> Self {
        let description = description.into();
        match self {
            Output::Text(mut text) => {
                text.description = Some(description);
                Output::Text(text)
            }
            Output::DataFrame(mut df) => {
                df.description = Some(description);
                Output::DataFrame(df)
            }
        }
    }

    /// Get the nickname from the output, if any.
    pub fn nickname(&self) -> Option<&str> {
        match self {
            Output::Text(text) => text.nickname.as_deref(),
            Output::DataFrame(df) => df.nickname.as_deref(),
        }
    }

    /// Set a short, no-spaces identifier for use in formulas (chainable).
    pub fn with_nickname(self, nickname: impl Into<String>) -> Self {
        let nickname = nickname.into();
        match self {
            Output::Text(mut text) => {
                text.nickname = Some(nickname);
                Output::Text(text)
            }
            Output::DataFrame(mut df) => {
                df.nickname = Some(nickname);
                Output::DataFrame(df)
            }
        }
    }
}

impl From<Text> for Output {
    fn from(text: Text) -> Self {
        Output::Text(text)
    }
}

impl From<DataFrame> for Output {
    fn from(df: DataFrame) -> Self {
        Output::DataFrame(df)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_builder_chains_all_fields() {
        let output = Output::text("hello")
            .with_label(Label::new("greeting"))
            .with_source("test")
            .with_store(true)
            .with_description("a greeting")
            .with_nickname("hi");

        assert_eq!(output.label().map(|l| l.0.as_str()), Some("greeting"));
        assert_eq!(output.source(), Some("test"));
        assert_eq!(output.store(), Some(true));
        assert_eq!(output.description(), Some("a greeting"));
        assert_eq!(output.nickname(), Some("hi"));
    }

    #[test]
    fn from_text_and_data_frame_wraps_in_output() {
        let text_output: Output = Text::new("hi").into();
        assert!(matches!(text_output, Output::Text(_)));

        let df = DataFrame::new(Vec::new(), serde_json::Value::Null, None);
        let df_output: Output = df.into();
        assert!(matches!(df_output, Output::DataFrame(_)));
    }
}
