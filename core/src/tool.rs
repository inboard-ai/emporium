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
