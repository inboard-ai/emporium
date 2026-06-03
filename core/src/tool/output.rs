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

    /// Create a new columnar output from JSON data (encodes to Arrow IPC).
    /// Fallible because encoding can fail on malformed data; the host wire path
    /// constructs [`DataFrame`] directly from an already-encoded buffer.
    pub fn columnar(
        data: serde_json::Value,
        schema: Vec<column::Def>,
        metadata: Option<serde_json::Value>,
    ) -> Result<Self, crate::Error> {
        Ok(Output::DataFrame(DataFrame::from_json(schema, data, metadata)?))
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

        let df = DataFrame::new(Vec::new(), Vec::new(), None);
        let df_output: Output = df.into();
        assert!(matches!(df_output, Output::DataFrame(_)));
    }

    #[test]
    fn output_label_returns_inner_from_both_variants() {
        let text = Output::text("hi").with_label(Label::new("L1"));
        assert_eq!(text.label().map(|l| l.0.as_str()), Some("L1"));

        let df = Output::columnar(serde_json::Value::Null, Vec::new(), None)
            .unwrap()
            .with_label(Label::new("L2"));
        assert_eq!(df.label().map(|l| l.0.as_str()), Some("L2"));
    }

    #[test]
    fn output_source_returns_inner_from_both_variants() {
        let text = Output::text("hi").with_source("text-src");
        assert_eq!(text.source(), Some("text-src"));

        let df = Output::columnar(serde_json::Value::Null, Vec::new(), None)
            .unwrap()
            .with_source("df-src");
        assert_eq!(df.source(), Some("df-src"));
    }

    #[test]
    fn output_description_returns_inner_from_both_variants() {
        let text = Output::text("hi").with_description("text-desc");
        assert_eq!(text.description(), Some("text-desc"));

        let df = Output::columnar(serde_json::Value::Null, Vec::new(), None)
            .unwrap()
            .with_description("df-desc");
        assert_eq!(df.description(), Some("df-desc"));
    }

    #[test]
    fn output_nickname_returns_inner_from_both_variants() {
        let text = Output::text("hi").with_nickname("text_nick");
        assert_eq!(text.nickname(), Some("text_nick"));

        let df = Output::columnar(serde_json::Value::Null, Vec::new(), None)
            .unwrap()
            .with_nickname("df_nick");
        assert_eq!(df.nickname(), Some("df_nick"));
    }

    #[test]
    fn output_store_override_returns_inner_from_both_variants() {
        let text = Output::text("hi").with_store(true);
        assert_eq!(text.store(), Some(true));

        let df = Output::columnar(serde_json::Value::Null, Vec::new(), None)
            .unwrap()
            .with_store(false);
        assert_eq!(df.store(), Some(false));
    }

    #[test]
    fn with_source_preserves_other_fields_on_text() {
        let text = Output::text("body")
            .with_label(Label::new("lab"))
            .with_description("desc")
            .with_nickname("nick")
            .with_store(true)
            .with_source("src");
        assert_eq!(text.source(), Some("src"));
        assert_eq!(text.label().map(|l| l.0.as_str()), Some("lab"));
        assert_eq!(text.description(), Some("desc"));
        assert_eq!(text.nickname(), Some("nick"));
        assert_eq!(text.store(), Some(true));
        match text {
            Output::Text(t) => assert_eq!(t.content, "body"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn with_source_preserves_other_fields_on_data_frame() {
        let col = column::Def {
            name: "k".to_string(),
            alias: "k".to_string(),
            dtype: "string".to_string(),
        };
        let df = Output::columnar(serde_json::json!([{"k": "v"}]), vec![col], None)
            .unwrap()
            .with_label(Label::new("lab"))
            .with_description("desc")
            .with_nickname("nick")
            .with_store(false)
            .with_source("src");
        assert_eq!(df.source(), Some("src"));
        assert_eq!(df.label().map(|l| l.0.as_str()), Some("lab"));
        assert_eq!(df.description(), Some("desc"));
        assert_eq!(df.nickname(), Some("nick"));
        assert_eq!(df.store(), Some(false));
        match df {
            Output::DataFrame(frame) => assert_eq!(frame.to_dataframe().unwrap().height(), 1),
            _ => panic!("expected DataFrame variant"),
        }
    }

    #[test]
    fn data_frame_output_is_serde_roundtripable() {
        let col = column::Def {
            name: "k".to_string(),
            alias: "K".to_string(),
            dtype: "string".to_string(),
        };
        let output = Output::columnar(serde_json::json!([{"k": "v"}]), vec![col], None)
            .unwrap()
            .with_label(Label::new("L"))
            .with_description("d")
            .with_nickname("n");
        let encoded = serde_json::to_string(&output).unwrap();
        let decoded: Output = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, Output::DataFrame(_)));
        assert_eq!(decoded.label().map(|l| l.0.as_str()), Some("L"));
        assert_eq!(decoded.description(), Some("d"));
        assert_eq!(decoded.nickname(), Some("n"));
    }

    #[test]
    fn columnar_constructs_data_frame_variant() {
        let col = column::Def {
            name: "k".to_string(),
            alias: "K".to_string(),
            dtype: "string".to_string(),
        };
        let output = Output::columnar(serde_json::json!([{"k": "v"}]), vec![col], None).unwrap();
        match output {
            Output::DataFrame(df) => {
                assert_eq!(df.schema.len(), 1);
                assert_eq!(df.schema[0].name, "k");
            }
            _ => panic!("expected DataFrame variant"),
        }
    }

    #[test]
    fn non_exhaustive_match_requires_catch_all() {
        // `tool::Output` is `#[non_exhaustive]` but matches within the same
        // crate ARE exhaustive (2 variants). This test documents the pattern
        // downstream crates would use — a catch-all `other =>` arm. Clippy
        // flags the arm as unreachable from within this crate.
        let output = Output::text("hi");
        #[allow(unreachable_patterns)]
        let kind = match &output {
            Output::Text(_) => "text",
            Output::DataFrame(_) => "dataframe",
            other => {
                let _ = other;
                "unknown"
            }
        };
        assert_eq!(kind, "text");
    }
}
