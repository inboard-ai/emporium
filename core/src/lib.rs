//! Core data types for the Emporium extension framework

use polars_core::prelude::*;
use serde::{Deserialize, Serialize};

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
        /// The tool to execute
        tool_id: String,
        /// The tool input as a JSON value
        params: serde_json::Value,
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
    pub fn execute_tool(tool_id: String, params: serde_json::Value, correlation_id: Option<String>) -> Self {
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
    pub schema: serde_json::Value,
}

/// Tool execution result with type safety
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    /// Text-based result
    Text(String),
    /// Columnar data that can be converted to DataFrame on the client
    Columns {
        /// The column data
        columns: Vec<Column>,
        /// The schema definition for the columns
        schema: Schema,
    },
}

impl ToolResult {
    /// Create a new text result
    pub fn text<T: Into<String>>(text: T) -> Result<Self, CoreError> {
        Ok(ToolResult::Text(text.into()))
    }

    /// Create columnar data that can be converted to DataFrame on the client
    pub fn columnar(data: serde_json::Value, column_defs: Schema) -> Result<Self, CoreError> {
        // Helper to convert string dtype to Polars DataType
        fn to_dtype(dtype: &str) -> DataType {
            match dtype {
                "string" => DataType::String,
                "number" | "float" => DataType::Float64,
                "integer" | "int" => DataType::Int64,
                "boolean" | "bool" => DataType::Boolean,
                "date" => DataType::Date,
                "datetime" => DataType::Datetime(TimeUnit::Microseconds, None),
                _ => DataType::String, // Default to string for unknown types
            }
        }

        // Validate that data is an array
        let arr = data
            .as_array()
            .ok_or_else(|| CoreError::Custom("Data must be a JSON array".to_string()))?;

        if arr.is_empty() {
            // Return empty columns vector with schema
            return Ok(ToolResult::Columns {
                columns: Vec::new(),
                schema: column_defs,
            });
        }

        // Create Series for each column based on schema
        let mut columns_vec = Vec::new();

        for col_def in &column_defs {
            let dtype = to_dtype(&col_def.dtype);
            let alias = col_def.alias.clone().into();
            let series = match dtype {
                DataType::String => {
                    let values: Vec<Option<String>> = arr
                        .iter()
                        .map(|item| {
                            item.get(&col_def.name).and_then(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Null => None,
                                other => Some(other.to_string()),
                            })
                        })
                        .collect();
                    Series::new(alias, values)
                }
                DataType::Float64 => {
                    let values: Vec<Option<f64>> = arr
                        .iter()
                        .map(|item| item.get(&col_def.name).and_then(|v| v.as_f64()))
                        .collect();
                    Series::new(alias, values)
                }
                DataType::Int64 => {
                    let values: Vec<Option<i64>> = arr
                        .iter()
                        .map(|item| item.get(&col_def.name).and_then(|v| v.as_i64()))
                        .collect();
                    Series::new(alias, values)
                }
                DataType::Boolean => {
                    let values: Vec<Option<bool>> = arr
                        .iter()
                        .map(|item| item.get(&col_def.name).and_then(|v| v.as_bool()))
                        .collect();
                    Series::new(alias, values)
                }
                _ => {
                    // Default to string representation for other types
                    let values: Vec<Option<String>> = arr
                        .iter()
                        .map(|item| {
                            item.get(&col_def.name).and_then(|v| match v {
                                serde_json::Value::Null => None,
                                other => Some(other.to_string()),
                            })
                        })
                        .collect();
                    Series::new(alias, values)
                }
            };
            columns_vec.push(Column::Series(series.into()));
        }

        Ok(ToolResult::Columns {
            columns: columns_vec,
            schema: column_defs,
        })
    }
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
        result: Result<ToolResult, E>,
        correlation_id: Option<String>,
    },

    /// Error response
    Error {
        message: String,
        correlation_id: Option<String>,
    },
}
