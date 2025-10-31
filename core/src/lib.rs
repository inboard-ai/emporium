//! Core data types for the Emporium extension framework

use polars_core::prelude::*;
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
        /// The tool to execute
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

/// Tool execution result with type safety
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    /// Text-based result
    Text(String),
    // Columns {
    //     /// The column data
    //     columns: Vec<Column>,
    //     /// The schema definition for the columns
    //     schema: Schema,
    //     /// Any metadata associated with the result
    //     metadata: Option<Value>,
    // },
    /// Columnar data that can be converted to DataFrame on the client
    DataFrame(ProtoDataFrame),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoDataFrame {
    pub schema: Schema,
    pub data: Value,
    pub metadata: Option<Value>,
}

impl ProtoDataFrame {
    pub fn to_dataframe(self) -> Result<DataFrame, CoreError> {
        // Helper to convert string dtype to Polars DataType
        fn to_dtype(dtype: &str) -> DataType {
            match dtype {
                "string" => DataType::String,
                "number" | "float" => DataType::Float64,
                "integer" | "int" => DataType::Int64,
                "boolean" | "bool" => DataType::Boolean,
                "date" => DataType::Date,
                "datetime" => DataType::Datetime(TimeUnit::Milliseconds, None),
                _ => DataType::String, // Default to string for unknown types
            }
        }

        // Helper to parse a value as f64, handling both number and string representations
        fn parse_as_f64(value: &serde_json::Value) -> Option<f64> {
            match value {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.parse::<f64>().ok(),
                _ => None,
            }
        }

        // Helper to parse a value as i64, handling both number and string representations
        fn parse_as_i64(value: &serde_json::Value) -> Option<i64> {
            match value {
                serde_json::Value::Number(n) => n.as_i64(),
                serde_json::Value::String(s) => s.parse::<i64>().ok(),
                _ => None,
            }
        }

        // Helper to parse a value as bool, handling various representations
        fn parse_as_bool(value: &serde_json::Value) -> Option<bool> {
            match value {
                serde_json::Value::Bool(b) => Some(*b),
                serde_json::Value::String(s) => match s.to_lowercase().as_str() {
                    "true" | "1" | "yes" => Some(true),
                    "false" | "0" | "no" => Some(false),
                    _ => None,
                },
                serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0),
                _ => None,
            }
        }

        match self.data {
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Ok(DataFrame::empty());
                }

                // Create Series for each column based on schema
                let mut columns_vec = Vec::new();

                for col_def in &self.schema {
                    let dtype = to_dtype(&col_def.dtype);
                    let alias = col_def.alias.clone().into();

                    let series = match dtype {
                        DataType::String | DataType::Date => {
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
                                .map(|item| item.get(&col_def.name).and_then(|v| parse_as_f64(v)))
                                .collect();
                            Series::new(alias, values)
                        }
                        DataType::Int64 => {
                            let values: Vec<Option<i64>> = arr
                                .iter()
                                .map(|item| item.get(&col_def.name).and_then(|v| parse_as_i64(v)))
                                .collect();
                            Series::new(alias, values)
                        }
                        DataType::Boolean => {
                            let values: Vec<Option<bool>> = arr
                                .iter()
                                .map(|item| item.get(&col_def.name).and_then(|v| parse_as_bool(v)))
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

                DataFrame::new(columns_vec)
                    .map_err(|e| CoreError::DataFrameError(format!("Failed to create DataFrame: {}", e)))
            }

            Value::Object(obj) => {
                // For object format, assume it's column-oriented data
                // e.g., {"col1": [1,2,3], "col2": ["a","b","c"]}
                let mut columns_vec = Vec::new();

                // Track the maximum number of rows across all columns
                let mut max_rows = 0usize;

                for col_def in &self.schema {
                    let alias = col_def.alias.clone().into();
                    let dtype = to_dtype(&col_def.dtype);

                    if let Some(col_data) = obj.get(&col_def.name) {
                        let series = match col_data {
                            Value::Array(values) => {
                                max_rows = max_rows.max(values.len());

                                match dtype {
                                    DataType::String | DataType::Date => {
                                        let parsed: Vec<Option<String>> = values
                                            .iter()
                                            .map(|v| match v {
                                                Value::Null => None,
                                                Value::String(s) => Some(s.clone()),
                                                other => Some(other.to_string()),
                                            })
                                            .collect();
                                        Series::new(alias, parsed)
                                    }
                                    DataType::Float64 => {
                                        let parsed: Vec<Option<f64>> = values.iter().map(|v| parse_as_f64(v)).collect();
                                        Series::new(alias, parsed)
                                    }
                                    DataType::Int64 => {
                                        let parsed: Vec<Option<i64>> = values.iter().map(|v| parse_as_i64(v)).collect();
                                        Series::new(alias, parsed)
                                    }
                                    DataType::Boolean => {
                                        let parsed: Vec<Option<bool>> =
                                            values.iter().map(|v| parse_as_bool(v)).collect();
                                        Series::new(alias, parsed)
                                    }
                                    _ => {
                                        // Default to string
                                        let parsed: Vec<Option<String>> = values
                                            .iter()
                                            .map(|v| match v {
                                                Value::Null => None,
                                                Value::String(s) => Some(s.clone()),
                                                other => Some(other.to_string()),
                                            })
                                            .collect();
                                        Series::new(alias, parsed)
                                    }
                                }
                            }
                            // If it's not an array, treat it as a single value repeated for all rows
                            // This will be adjusted after we know the max_rows
                            _ => {
                                // For now, create a single-element series
                                match dtype {
                                    DataType::String | DataType::Date => {
                                        let value = match col_data {
                                            Value::Null => None,
                                            Value::String(s) => Some(s.clone()),
                                            other => Some(other.to_string()),
                                        };
                                        Series::new(alias, vec![value])
                                    }
                                    DataType::Float64 => Series::new(alias, vec![parse_as_f64(col_data)]),
                                    DataType::Int64 => Series::new(alias, vec![parse_as_i64(col_data)]),
                                    DataType::Boolean => Series::new(alias, vec![parse_as_bool(col_data)]),
                                    _ => {
                                        let value = match col_data {
                                            Value::Null => None,
                                            Value::String(s) => Some(s.clone()),
                                            other => Some(other.to_string()),
                                        };
                                        Series::new(alias, vec![value])
                                    }
                                }
                            }
                        };
                        columns_vec.push(Column::Series(series.into()));
                    } else {
                        // Column not found in data, create empty column with nulls
                        // We'll resize it to max_rows after processing all columns
                        let series = match dtype {
                            DataType::String | DataType::Date => Series::new(alias, Vec::<Option<String>>::new()),
                            DataType::Float64 => Series::new(alias, Vec::<Option<f64>>::new()),
                            DataType::Int64 => Series::new(alias, Vec::<Option<i64>>::new()),
                            DataType::Boolean => Series::new(alias, Vec::<Option<bool>>::new()),
                            _ => Series::new(alias, Vec::<Option<String>>::new()),
                        };
                        columns_vec.push(Column::Series(series.into()));
                    }
                }

                // If we have columns, ensure they all have the same length
                // This handles cases where some columns might have been single values or missing
                if !columns_vec.is_empty() && max_rows > 0 {
                    for col in columns_vec.iter_mut() {
                        if let Column::Series(series) = col {
                            let current_len = series.len();
                            if current_len == 0 {
                                // Create a series of nulls with the right length
                                let dtype = series.dtype();
                                let name = series.name().clone();
                                let new_series = match dtype {
                                    DataType::String => Series::new(name, vec![Option::<String>::None; max_rows]),
                                    DataType::Float64 => Series::new(name, vec![Option::<f64>::None; max_rows]),
                                    DataType::Int64 => Series::new(name, vec![Option::<i64>::None; max_rows]),
                                    DataType::Boolean => Series::new(name, vec![Option::<bool>::None; max_rows]),
                                    _ => Series::new(name, vec![Option::<String>::None; max_rows]),
                                };
                                *series = new_series.into();
                            } else if current_len == 1 && max_rows > 1 {
                                // Repeat the single value for all rows
                                // This is a broadcast operation
                                let extended = series.new_from_index(0, max_rows);
                                *series = extended.into();
                            }
                            // If current_len matches max_rows, nothing to do
                        }
                    }
                }

                DataFrame::new(columns_vec)
                    .map_err(|e| CoreError::DataFrameError(format!("Failed to create DataFrame: {}", e)))
            }

            Value::Null => Ok(DataFrame::empty()),

            _ => Err(CoreError::DataFrameError(
                "Data must be an array or object to convert to DataFrame".to_string(),
            )),
        }
    }
}

impl ToolResult {
    /// Create a new text result
    pub fn text<T: Into<String>>(text: T) -> Self {
        ToolResult::Text(text.into())
    }

    /// Create columnar data that can be converted to DataFrame on the client
    pub fn columnar(data: serde_json::Value, schema: Schema, metadata: Option<serde_json::Value>) -> Self {
        ToolResult::DataFrame(ProtoDataFrame { schema, data, metadata })
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
