//! Core data types for the Emporium extension framework

use polars_core::prelude::*;
use serde::{Deserialize, Serialize};

/// Core error type for data operations
#[derive(Debug, Clone, thiserror::Error)]
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

/// Tool execution result with type safety
#[derive(Debug, Clone)]
pub enum ToolResult {
    /// Text-based result
    Text(String),
    /// DataFrame result with schema
    DataFrame(DataFrame),
}

impl ToolResult {
    /// Create a new text result
    pub fn text<T: Into<String>>(text: T) -> Result<Self, CoreError> {
        Ok(ToolResult::Text(text.into()))
    }

    /// Create a new DataFrame result
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
            // Create empty DataFrame with schema
            let polars_schema = polars_core::prelude::Schema::from_iter(
                column_defs
                    .iter()
                    .map(|col| (col.alias.clone().into(), to_dtype(&col.dtype))),
            );
            let df = DataFrame::empty_with_schema(&polars_schema);
            return Ok(ToolResult::DataFrame(df));
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

        let dataframe = DataFrame::new(columns_vec)
            .map_err(|e| CoreError::DataFrameError(format!("Failed to create DataFrame: {}", e)))?;

        Ok(ToolResult::DataFrame(dataframe))
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
    ToolList(Vec<ToolInfo>),

    /// Detailed information about a specific tool
    ToolDetails(ToolInfo),

    /// Result from tool execution
    #[serde(skip)]
    ToolResult {
        tool_id: String,
        result: Result<ToolResult, E>,
    },

    /// Generic data response (for backwards compatibility)
    Data(String),

    /// Error response
    Error(String),
}
