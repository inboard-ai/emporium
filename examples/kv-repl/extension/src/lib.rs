//! Key-Value Store Extension
//!
//! A simple in-memory key-value store extension for Emporium.

use std::cell::RefCell;
use std::collections::HashMap;

use emporium_core::{tool::ToolResult, Command, Response, ToolInfo};
use serde_json::{json, Value};

wit_bindgen::generate!({
    world: "extension-world",
    path: "wit",
});

use exports::emporium::extensions::extension::{Guest, GuestInstance, Instance, Metadata};

struct Component;

export!(Component);

impl Guest for Component {
    type Instance = KvInstance;

    fn get_metadata() -> Metadata {
        Metadata {
            id: "kv".to_string(),
            name: "Key-Value Store".to_string(),
            version: "0.1.0".to_string(),
            description: "A simple in-memory key-value store".to_string(),
        }
    }
}

struct KvInstance {
    store: RefCell<HashMap<String, String>>,
}

impl GuestInstance for KvInstance {
    fn new(_config: String) -> Instance {
        Instance::new(KvInstance {
            store: RefCell::new(HashMap::new()),
        })
    }

    fn update(&self, command: String) -> Result<String, String> {
        let cmd: Command = serde_json::from_str(&command).map_err(|e| e.to_string())?;

        let response = match cmd {
            Command::ListTools { correlation_id } => Response::ToolList {
                tools: tool_list(),
                correlation_id,
            },
            Command::GetToolDetails {
                tool_id,
                correlation_id,
            } => {
                if let Some(tool) = tool_list().into_iter().find(|t| t.id == tool_id) {
                    Response::ToolDetails {
                        tool_id,
                        tool_info: tool,
                        correlation_id,
                    }
                } else {
                    Response::Error {
                        message: format!("Unknown tool: {}", tool_id),
                        correlation_id,
                    }
                }
            }
            Command::ExecuteTool {
                name,
                params,
                correlation_id,
            } => {
                let result = self.execute_tool(&name, params);
                Response::ToolResult {
                    name,
                    result,
                    correlation_id,
                }
            }
            Command::Custom { correlation_id, .. } => Response::Error {
                message: "Custom commands not supported".to_string(),
                correlation_id,
            },
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn view(&self) -> String {
        serde_json::to_string(&*self.store.borrow()).unwrap_or_default()
    }
}

fn tool_list() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            id: "get".to_string(),
            name: "get".to_string(),
            description: "Get the value for a key".to_string(),
            schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to retrieve" }
                },
                "required": ["key"]
            }),
        },
        ToolInfo {
            id: "set".to_string(),
            name: "set".to_string(),
            description: "Set a key to a value".to_string(),
            schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to set" },
                    "value": { "type": "string", "description": "The value to store" }
                },
                "required": ["key", "value"]
            }),
        },
        ToolInfo {
            id: "delete".to_string(),
            name: "delete".to_string(),
            description: "Delete a key".to_string(),
            schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The key to delete" }
                },
                "required": ["key"]
            }),
        },
        ToolInfo {
            id: "get_all".to_string(),
            name: "get_all".to_string(),
            description: "List all key-value pairs".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            id: "clear".to_string(),
            name: "clear".to_string(),
            description: "Clear all entries".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            id: "stats".to_string(),
            name: "stats".to_string(),
            description: "Get store statistics".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

impl KvInstance {
    fn execute_tool(&self, name: &str, params: Value) -> Result<ToolResult, emporium_core::CoreError> {
        match name {
            "get" => {
                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| emporium_core::CoreError::custom("Missing 'key' parameter"))?;

                match self.store.borrow().get(key) {
                    Some(value) => Ok(ToolResult::text(value)),
                    None => Ok(ToolResult::text(format!("Key '{}' not found", key))),
                }
            }
            "set" => {
                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| emporium_core::CoreError::custom("Missing 'key' parameter"))?;
                let value = params
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| emporium_core::CoreError::custom("Missing 'value' parameter"))?;

                self.store
                    .borrow_mut()
                    .insert(key.to_string(), value.to_string());
                Ok(ToolResult::text(""))
            }
            "delete" => {
                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| emporium_core::CoreError::custom("Missing 'key' parameter"))?;

                match self.store.borrow_mut().remove(key) {
                    Some(_) => Ok(ToolResult::text("")),
                    None => Ok(ToolResult::text(format!("Key '{}' not found", key))),
                }
            }
            "get_all" => {
                let store = self.store.borrow();
                if store.is_empty() {
                    Ok(ToolResult::text("(empty)"))
                } else {
                    let entries: Vec<String> =
                        store.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                    Ok(ToolResult::text(entries.join("\n")))
                }
            }
            "clear" => {
                self.store.borrow_mut().clear();
                Ok(ToolResult::text(""))
            }
            "stats" => {
                let count = self.store.borrow().len();
                let msg = if count == 1 {
                    "1 entry".to_string()
                } else {
                    format!("{} entries", count)
                };
                Ok(ToolResult::text(msg))
            }
            _ => Err(emporium_core::CoreError::custom(format!(
                "Unknown tool: {}",
                name
            ))),
        }
    }
}
