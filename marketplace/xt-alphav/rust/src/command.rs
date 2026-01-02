//! Command handling for emporium protocol

use alphav::tool_use;
use emporium_core::{Command, CoreError, Response};
use serde_json::json;

/// Handle emporium protocol commands and return appropriate responses
pub async fn respond<Client: alphav::Request>(
    client: &alphav::AlphaVantage<Client>,
    cmd: Command,
) -> Response<CoreError> {
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
            name,
            params,
            correlation_id,
        } => {
            let request = json!({
                "tool": name.clone(),
                "params": params
            });

            match tool_use::call_tool(client, request).await {
                Ok(tool_result) => Response::ToolResult {
                    name,
                    result: Ok(tool_result),
                    correlation_id,
                },
                Err(e) => Response::ToolResult {
                    name,
                    result: Err(CoreError::Custom(format!("{:?}", e))),
                    correlation_id,
                },
            }
        }
        Command::Custom {
            command,
            correlation_id,
        } => {
            emporium_core::tool::perform(command, correlation_id, |request| async {
                tool_use::call_tool(client, request)
                    .await
                    .map_err(|e| CoreError::Custom(format!("{:?}", e)))
            })
            .await
        }
    }
}
