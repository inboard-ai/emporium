//! Core data types for the Emporium extension framework

pub mod tool;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Core error type for data operations
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum CoreError {
    #[error("JSON error: {0}")]
    JsonError(String),
    #[error("DataFrame error: {0}")]
    DataFrameError(String),
    #[error("{0}")]
    Custom(String),
}

impl CoreError {
    pub fn custom<T: Into<String>>(msg: T) -> Self {
        CoreError::Custom(msg.into())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(err: serde_json::Error) -> Self {
        CoreError::JsonError(err.to_string())
    }
}

impl From<String> for CoreError {
    fn from(err: String) -> Self {
        CoreError::Custom(err)
    }
}

impl From<&str> for CoreError {
    fn from(err: &str) -> Self {
        CoreError::Custom(err.to_string())
    }
}

/// Column definition for DataFrame schemas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    /// Original column name from the data source
    pub name: String,
    /// Human-friendly display name
    pub alias: String,
    /// Data type (e.g., "string", "number", "date")
    pub dtype: String,
}

/// Schema definition - alias for Vec<ColumnDef>
pub type Schema = Vec<ColumnDef>;

pub type Id = String;

/// A command sent TO an extension
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Command {
    /// List all available tools the extension provides
    ListTools {
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },

    /// Get detailed information about a specific tool
    GetToolDetails {
        /// The tool identifier
        tool_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },

    /// Execute a tool with the provided input
    ExecuteTool {
        /// The identifier of the tool to execute
        tool_id: String,
        /// The tool input as a JSON value
        params: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },

    /// Any custom command
    Custom {
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
}

impl Command {
    /// Create a ListTools command with optional correlation ID
    pub fn list_tools(correlation_id: Option<String>) -> Self {
        Self::ListTools { correlation_id }
    }

    /// Create a GetToolDetails command with optional correlation ID
    pub fn get_tool_details(tool_id: String, correlation_id: Option<String>) -> Self {
        Self::GetToolDetails {
            tool_id,
            correlation_id,
        }
    }

    /// Create an ExecuteTool command with optional correlation ID
    pub fn execute_tool(tool_id: String, params: Value, correlation_id: Option<String>) -> Self {
        Self::ExecuteTool {
            tool_id,
            params,
            correlation_id,
        }
    }

    /// Extract the correlation ID from any command variant
    pub fn correlation_id(&self) -> Option<&String> {
        match self {
            Self::ListTools { correlation_id } => correlation_id.as_ref(),
            Self::GetToolDetails { correlation_id, .. } => correlation_id.as_ref(),
            Self::ExecuteTool { correlation_id, .. } => correlation_id.as_ref(),
            Self::Custom { correlation_id, .. } => correlation_id.as_ref(),
        }
    }
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
    pub schema: Value,
}


/// A response received FROM an extension
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Response<E = CoreError> {
    /// Extension metadata
    Metadata {
        id: String,
        name: String,
        version: String,
        description: String,
    },

    /// List of available tools
    ToolList {
        tools: Vec<ToolInfo>,
        correlation_id: Option<String>,
    },

    /// Detailed information about a specific tool
    ToolDetails {
        tool_id: String,
        tool_info: ToolInfo,
        correlation_id: Option<String>,
    },

    /// Result from tool execution
    ToolResult {
        tool_id: String,
        tool_name: String,
        result: Result<tool::ToolResult, E>,
        correlation_id: Option<String>,
    },

    /// Error response
    Error {
        message: String,
        correlation_id: Option<String>,
    },
}
