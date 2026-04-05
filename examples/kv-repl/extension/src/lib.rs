//! Key-Value Store Extension
//!
//! A simple in-memory key-value store extension for Emporium. Exports the
//! `extension` and `tool-provider` interfaces of the `tool-extension` world.

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{Value, json};

wit_bindgen::generate!({
    world: "tool-extension",
    path: "wit",
});

use emporium::extensions::host_events::{self, Event, Progress};
use exports::emporium::extensions::extension::{Guest as ExtensionGuest, Metadata};
use exports::emporium::extensions::tool_provider::{Guest as ToolProviderGuest, TextOutput, ToolInfo, ToolOutput};

struct Component;

export!(Component);

thread_local! {
    /// Component-level key-value store. WASM is single-threaded per component
    /// instance, so a `thread_local!` + `RefCell` suffices — no contention.
    static KV: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

impl ExtensionGuest for Component {
    fn get_metadata() -> Metadata {
        Metadata {
            id: "kv".to_string(),
            name: "Key-Value Store".to_string(),
            version: "0.1.0".to_string(),
            description: "A simple in-memory key-value store".to_string(),
        }
    }

    fn init(config: String) -> Result<(), String> {
        let _ = config;
        Ok(())
    }

    fn view() -> String {
        KV.with_borrow(|kv| serde_json::to_string(kv).unwrap_or_default())
    }
}

impl ToolProviderGuest for Component {
    fn list_tools() -> Vec<ToolInfo> {
        tool_list()
    }

    fn execute_tool(name: String, params: String) -> Result<ToolOutput, String> {
        // Emit a progress event so the host can surface activity in UI/logs.
        notify_progress(&name);
        let parsed: Value = serde_json::from_str(&params).map_err(|err| format!("invalid params JSON: {err}"))?;
        execute_tool(&name, &parsed)
    }
}

/// Push a `host-events::progress` notification announcing that `tool` is running.
fn notify_progress(tool: &str) {
    host_events::notify(&Event::Progress(Progress {
        percent: None,
        message: format!("Running {tool}"),
    }));
}

/// Return the static tool manifest advertised by this extension.
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
            })
            .to_string(),
            cacheable: false,
            activity: None,
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
            })
            .to_string(),
            cacheable: false,
            activity: None,
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
            })
            .to_string(),
            cacheable: false,
            activity: None,
        },
        ToolInfo {
            id: "get_all".to_string(),
            name: "get_all".to_string(),
            description: "List all key-value pairs".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            })
            .to_string(),
            cacheable: false,
            activity: None,
        },
        ToolInfo {
            id: "clear".to_string(),
            name: "clear".to_string(),
            description: "Clear all entries".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            })
            .to_string(),
            cacheable: false,
            activity: None,
        },
        ToolInfo {
            id: "stats".to_string(),
            name: "stats".to_string(),
            description: "Get store statistics".to_string(),
            schema: json!({
                "type": "object",
                "properties": {}
            })
            .to_string(),
            cacheable: false,
            activity: None,
        },
    ]
}

/// Dispatch a tool invocation against the thread-local KV store.
fn execute_tool(name: &str, params: &Value) -> Result<ToolOutput, String> {
    match name {
        "get" => {
            let key = require_str(params, "key")?;
            let output = KV.with_borrow(|kv| match kv.get(key) {
                Some(value) => text_output(value.clone()),
                None => text_output(format!("Key '{key}' not found")),
            });
            Ok(output)
        }
        "set" => {
            let key = require_str(params, "key")?;
            let value = require_str(params, "value")?;
            KV.with_borrow_mut(|kv| kv.insert(key.to_string(), value.to_string()));
            Ok(text_output(String::new()))
        }
        "delete" => {
            let key = require_str(params, "key")?;
            let output = KV.with_borrow_mut(|kv| match kv.remove(key) {
                Some(_) => text_output(String::new()),
                None => text_output(format!("Key '{key}' not found")),
            });
            Ok(output)
        }
        "get_all" => {
            let content = KV.with_borrow(|kv| {
                if kv.is_empty() {
                    "(empty)".to_string()
                } else {
                    kv.iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            });
            Ok(text_output(content))
        }
        "clear" => {
            KV.with_borrow_mut(|kv| kv.clear());
            Ok(text_output(String::new()))
        }
        "stats" => {
            let count = KV.with_borrow(|kv| kv.len());
            let message = if count == 1 {
                "1 entry".to_string()
            } else {
                format!("{count} entries")
            };
            Ok(text_output(message))
        }
        other => Err(format!("Unknown tool: {other}")),
    }
}

/// Extract a required string field from a params JSON object, returning an
/// error string if the field is missing or not a string.
fn require_str<'a>(params: &'a Value, field: &str) -> Result<&'a str, String> {
    params
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing '{field}' parameter"))
}

/// Wrap a string in a `ToolOutput::TextOutput` with no optional metadata set.
fn text_output(content: String) -> ToolOutput {
    ToolOutput::TextOutput(TextOutput {
        content,
        label: None,
        source: None,
        description: None,
        nickname: None,
        store: None,
    })
}
