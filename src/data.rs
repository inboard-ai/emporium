use futures::channel::mpsc;
use serde::{Deserialize, Serialize};

use crate::Error;

// Re-export all core types
pub use emporium_core::{
    ColumnDef, Schema, Id, Command, ToolInfo, ToolResult, CoreError,
};

/// Extended Response type with emporium-specific variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    #[serde(skip)]
    Connected(mpsc::UnboundedSender<Command>),

    /// Extension metadata
    Metadata {
        id: String,
        name: String,
        version: String,
        description: String,
    },

    /// List of available tools
    ToolList(Vec<ToolInfo>),

    /// Detailed information about a specific tool
    ToolDetails(ToolInfo),

    /// Result from tool execution
    #[serde(skip)]
    ToolResult {
        tool_id: String,
        result: Result<ToolResult, Error>,
    },

    /// Generic data response (for backwards compatibility)
    Data(String),

    /// Error response
    Error(String),
}
