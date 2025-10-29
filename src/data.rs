use futures::channel::mpsc;
use serde::{Deserialize, Serialize};

pub type Id = String;

/// A command sent TO an extension
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Command {
    /// List all available tools the extension provides
    ListTools,

    /// Get detailed information about a specific tool
    GetToolDetails {
        /// The tool identifier
        tool_id: String,
    },

    /// Execute a tool with the provided input
    ExecuteTool {
        /// The tool to execute
        tool_id: String,
        /// The tool input as a JSON value
        params: serde_json::Value,
    },

    /// Any custom command
    Custom(String),
}

/// Tool information provided by an extension
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Unique identifier for the tool
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what the tool does
    pub description: String,
    /// JSON Schema for the tool's parameters
    pub schema: serde_json::Value,
}

/// A response received FROM an extension
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
    ToolResult { tool_id: String, result: serde_json::Value },

    /// Generic data response (for backwards compatibility)
    Data(String),

    /// Error response
    Error(String),
}
