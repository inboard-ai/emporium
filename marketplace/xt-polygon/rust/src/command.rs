//! Command handling for emporium protocol

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Emporium protocol command types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Command {
    /// List all available tools
    ListTools,
    /// Get details for a specific tool
    GetToolDetails {
        /// The tool identifier to get details for
        tool_id: String,
    },
    /// Execute a tool with parameters
    ExecuteTool {
        /// The tool identifier to execute
        tool_id: String,
        /// Parameters to pass to the tool
        params: Value,
    },
    /// Custom command (for backwards compatibility)
    Custom(String),
}

/// Emporium protocol response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    /// List of available tools
    ToolList(Vec<polygon::tool_use::ToolInfo>),
    /// Details for a specific tool
    ToolDetails(polygon::tool_use::ToolInfo),
    /// Result from tool execution
    ToolResult {
        /// The tool that was executed
        tool_id: String,
        /// The result data from the tool
        result: Value,
    },
    /// Generic data response (for backwards compatibility)
    Data(String),
    /// Error response
    Error(String),
}

/// Handle emporium protocol commands and return appropriate responses
pub async fn respond<Client: polygon::Request>(client: &polygon::Polygon<Client>, cmd: Command) -> Response {
    match cmd {
        Command::ListTools => Response::ToolList(polygon::tool_use::list_tools()),
        Command::GetToolDetails { tool_id } => match polygon::tool_use::get_tool_details(&tool_id) {
            Some(tool) => Response::ToolDetails(tool),
            None => Response::Error(format!("Tool '{}' not found", tool_id)),
        },
        Command::ExecuteTool { tool_id, params } => {
            let request = json!({
                "tool": tool_id,
                "params": params
            });

            match polygon::tool_use::call_tool(client, request).await {
                Ok(result) => Response::ToolResult { tool_id, result },
                Err(e) => Response::Error(format!("Tool execution failed: {:?}", e)),
            }
        }
        Command::Custom(msg) => {
            // Try to parse as legacy tool call format
            if let Ok(request) = serde_json::from_str::<Value>(&msg) {
                if request.get("tool").is_some() {
                    match polygon::tool_use::call_tool(client, request).await {
                        Ok(result) => Response::Data(result.to_string()),
                        Err(e) => Response::Error(format!("Tool call failed: {:?}", e)),
                    }
                } else {
                    Response::Error("Invalid custom command format".to_string())
                }
            } else {
                Response::Error("Invalid custom command format".to_string())
            }
        }
    }
}
