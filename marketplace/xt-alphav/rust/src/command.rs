//! Command handling for emporium protocol

use emporium_core::{Command, Response, ToolResult, CoreError};
use serde_json::{Value, json};

/// Handle emporium protocol commands and return appropriate responses
pub async fn respond<Client: alphav::Request>(client: &alphav::AlphaVantage<Client>, cmd: Command) -> Response<CoreError> {
    match cmd {
        Command::ListTools => Response::ToolList(alphav::tool_use::list_tools()),
        Command::GetToolDetails { tool_id } => match alphav::tool_use::get_tool_details(&tool_id) {
            Some(tool) => Response::ToolDetails(tool),
            None => Response::Error(format!("Tool '{}' not found", tool_id)),
        },
        Command::ExecuteTool { tool_id, params } => {
            let request = json!({
                "tool": tool_id.clone(),
                "params": params
            });

            match alphav::tool_use::call_tool(client, request).await {
                Ok(result) => {
                    // Create ToolResult based on what alphav returned
                    let tool_result = match result {
                        alphav::tool_use::ToolCallResult::Text(text) => ToolResult::text(text),
                        alphav::tool_use::ToolCallResult::DataFrame { data, schema } => {
                            ToolResult::columnar(data, schema)
                        }
                    };

                    Response::ToolResult {
                        tool_id,
                        result: tool_result,
                    }
                }
                Err(e) => Response::Error(format!("Tool execution failed: {:?}", e)),
            }
        }
        Command::Custom(msg) => {
            // Try to parse as legacy tool call format
            if let Ok(request) = serde_json::from_str::<Value>(&msg) {
                if request.get("tool").is_some() {
                    match alphav::tool_use::call_tool(client, request).await {
                        Ok(result) => {
                            // Unpack the result and convert to string for legacy compatibility
                            let data_str = match result {
                                alphav::tool_use::ToolCallResult::Text(text) => text,
                                alphav::tool_use::ToolCallResult::DataFrame { data, .. } => data.to_string(),
                            };
                            Response::Data(data_str)
                        }
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
