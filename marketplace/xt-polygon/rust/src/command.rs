//! Command handling for emporium protocol

use emporium_core::{Command, CoreError, Response, ToolResult};
use polygon::tool_use;
use serde_json::{Value, json};

/// Handle emporium protocol commands and return appropriate responses
pub async fn respond<Client: polygon::Request>(client: &polygon::Polygon<Client>, cmd: Command) -> Response<CoreError> {
    match cmd {
        Command::ListTools { correlation_id } => Response::ToolList {
            tools: tool_use::list_tools(),
            correlation_id,
        },
        Command::GetToolDetails {
            tool_id,
            correlation_id,
        } => match tool_use::get_tool_details(&tool_id) {
            Some(tool) => Response::ToolDetails {
                tool_id,
                tool_info: tool,
                correlation_id,
            },
            None => Response::Error {
                message: format!("Tool '{}' not found", tool_id),
                correlation_id,
            },
        },
        Command::ExecuteTool {
            tool_id,
            params,
            correlation_id,
        } => {
            let request = json!({
                "tool": tool_id.clone(),
                "params": params
            });

            match tool_use::call_tool(client, request).await {
                Ok(result) => {
                    // Create ToolResult based on what polygon returned
                    let tool_result = match result {
                        tool_use::ToolCallResult::Text(text) => ToolResult::text(text),
                        tool_use::ToolCallResult::DataFrame { data, schema, metadata } => {
                            ToolResult::columnar(data, schema, metadata)
                        }
                    };

                    Response::ToolResult {
                        tool_id,
                        result: Ok(tool_result),
                        correlation_id,
                    }
                }
                Err(e) => Response::Error {
                    message: format!("Tool execution failed: {:?}", e),
                    correlation_id,
                },
            }
        }
        Command::Custom {
            command,
            correlation_id,
        } => {
            if let Ok(request) = serde_json::from_str::<Value>(&command) {
                if request.get("tool").is_some() {
                    match tool_use::call_tool(client, request).await {
                        // Create ToolResult based on what polygon returned
                        Ok(result) => {
                            let tool_result = match result {
                                tool_use::ToolCallResult::Text(text) => ToolResult::text(text),
                                tool_use::ToolCallResult::DataFrame { data, schema, metadata } => {
                                    ToolResult::columnar(data, schema, metadata)
                                }
                            };
                            Response::ToolResult {
                                tool_id: "custom".to_string(),
                                result: Ok(tool_result),
                                correlation_id,
                            }
                        }
                        Err(e) => Response::Error {
                            message: format!("Tool execution failed: {:?}", e),
                            correlation_id,
                        },
                    }
                } else {
                    Response::Error {
                        message: "Invalid custom command format".to_string(),
                        correlation_id,
                    }
                }
            } else {
                Response::Error {
                    message: "Invalid custom command format".to_string(),
                    correlation_id,
                }
            }
        }
    }
}
