//! Key-Value Store Extension
//!
//! A simple in-memory key-value store extension for Emporium. Exports the
//! `extension`, `tool-provider`, `block-provider`, and `formula-provider`
//! interfaces of the `full-extension` world, and imports `host-data` for
//! cursor-based streaming access to host-managed resources.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

wit_bindgen::generate!({
    world: "full-extension",
    path: "wit",
});

use emporium::extensions::host_data::Cursor;
use emporium::extensions::host_events::{self, Event, Progress};
use exports::emporium::extensions::block_provider::{BlockTypeInfo, Guest as BlockProviderGuest, PlanOutcome};
use exports::emporium::extensions::extension::{Guest as ExtensionGuest, Metadata};
use exports::emporium::extensions::formula_provider::{FormulaDef, Guest as FormulaProviderGuest};
use exports::emporium::extensions::tool_provider::{
    ColumnDef, DataFrameOutput, Guest as ToolProviderGuest, TextOutput, ToolInfo, ToolOutput,
};

struct Component;

export!(Component);

/// The sole block kind advertised by this extension.
const PREFIX_TRACKER_KIND: &str = "prefix-tracker";

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
        let parsed: Value = serde_json::from_str(&params).map_err(|err| format!("invalid params JSON: {err}"))?;
        execute_tool(&name, &parsed)
    }
}

impl BlockProviderGuest for Component {
    /// Advertise the block kinds this extension supports. Currently only the
    /// `prefix-tracker` demo block.
    fn get_block_types() -> Vec<BlockTypeInfo> {
        vec![BlockTypeInfo {
            kind: PREFIX_TRACKER_KIND.to_string(),
            name: "Prefix Tracker".to_string(),
            description: "Tracks a set of key prefixes and queries matching keys in the KV store.".to_string(),
        }]
    }

    /// Create a new block state from user input. The input is expected to be
    /// a JSON object of the form `{"prefixes": [...]}`. A missing or null
    /// `prefixes` field is treated as an empty list.
    fn create(kind: String, input: String) -> Result<String, String> {
        require_prefix_tracker(&kind)?;
        let prefixes = parse_prefix_list(&input)?;
        serialize_state(&prefixes)
    }

    /// Plan a block operation. The returned `PlanOutcome` describes the
    /// host-side effects: either a new state, a query plan, or both.
    fn plan(kind: String, state: String, operation: String, input: String) -> Result<PlanOutcome, String> {
        require_prefix_tracker(&kind)?;
        let mut prefixes = parse_state(&state)?;
        let input_value = parse_json_object(&input)?;

        match operation.as_str() {
            "add_prefix" => {
                let prefix = require_str(&input_value, "prefix")?.to_string();
                if !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
                Ok(PlanOutcome::StateUpdate(serialize_state(&prefixes)?))
            }
            "query" => Ok(PlanOutcome::Query(build_query_plan(&prefixes)?)),
            "rename_prefix" => {
                let old = require_str(&input_value, "old")?.to_string();
                let new = require_str(&input_value, "new")?.to_string();
                match prefixes.iter().position(|p| p == &old) {
                    Some(idx) => {
                        prefixes[idx] = new;
                        let new_state = serialize_state(&prefixes)?;
                        let plan = build_query_plan(&prefixes)?;
                        Ok(PlanOutcome::StateUpdateWithQuery((new_state, plan)))
                    }
                    None => Err(format!("prefix not found: {old}")),
                }
            }
            "analyze" => {
                notify_progress("analyze");
                let freq_json = analyze_keys()?;
                Ok(PlanOutcome::Computed(freq_json))
            }
            "add_and_analyze" => {
                notify_progress("add_and_analyze");
                let prefix = require_str(&input_value, "prefix")?.to_string();
                if !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
                let new_state = serialize_state(&prefixes)?;
                let freq_json = analyze_keys()?;
                Ok(PlanOutcome::StateUpdateWithComputed((new_state, freq_json)))
            }
            other => Err(format!("Unknown operation: {other}")),
        }
    }

    /// Validate a block state. The state must be a JSON object with a
    /// `prefixes` field that is an array of strings.
    fn validate(kind: String, state: String) -> Result<(), String> {
        require_prefix_tracker(&kind)?;
        let _ = parse_state(&state)?;
        Ok(())
    }
}

impl FormulaProviderGuest for Component {
    /// Advertise the formulas this extension supports. Each formula declares
    /// a JSON Schema describing its positional argument array.
    fn get_formulas() -> Vec<FormulaDef> {
        vec![
            FormulaDef {
                name: "KV_COUNT".to_string(),
                description: "Number of entries in the KV store.".to_string(),
                schema: json!({
                    "type": "array",
                    "items": [],
                    "minItems": 0,
                    "maxItems": 0
                })
                .to_string(),
            },
            FormulaDef {
                name: "KV_GET".to_string(),
                description: "Value for the given key, or null if absent.".to_string(),
                schema: json!({
                    "type": "array",
                    "items": [
                        { "type": "string", "description": "The key to look up" }
                    ],
                    "minItems": 1,
                    "maxItems": 1
                })
                .to_string(),
            },
            FormulaDef {
                name: "KV_EXISTS".to_string(),
                description: "Whether the given key is present in the KV store.".to_string(),
                schema: json!({
                    "type": "array",
                    "items": [
                        { "type": "string", "description": "The key to test" }
                    ],
                    "minItems": 1,
                    "maxItems": 1
                })
                .to_string(),
            },
        ]
    }

    /// Evaluate a formula against the thread-local KV store. `args` is a JSON
    /// array of positional arguments; the result is a JSON-encoded value.
    fn evaluate(name: String, args: String) -> Result<String, String> {
        let parsed: Value = serde_json::from_str(&args).map_err(|err| format!("invalid args JSON: {err}"))?;
        let items = parsed
            .as_array()
            .ok_or_else(|| "args must be a JSON array".to_string())?;
        match name.as_str() {
            "KV_COUNT" => {
                let count = KV.with_borrow(|kv| kv.len());
                Ok(json!(count).to_string())
            }
            "KV_GET" => {
                let key = arg_str(items, 0, "KV_GET expects 1 string argument: [key]")?;
                let value = KV.with_borrow(|kv| kv.get(key).cloned());
                Ok(json!(value).to_string())
            }
            "KV_EXISTS" => {
                let key = arg_str(items, 0, "KV_EXISTS expects 1 string argument: [key]")?;
                let exists = KV.with_borrow(|kv| kv.contains_key(key));
                Ok(json!(exists).to_string())
            }
            other => Err(format!("Unknown formula: {other}")),
        }
    }
}

/// Borrow a positional string argument from a parsed JSON args array,
/// returning `msg` if the slot is missing or not a string.
fn arg_str<'a>(items: &'a [Value], index: usize, msg: &str) -> Result<&'a str, String> {
    items.get(index).and_then(Value::as_str).ok_or_else(|| msg.to_string())
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
        ToolInfo {
            id: "list_keys".to_string(),
            name: "list_keys".to_string(),
            description: "List all keys as a single-column DataFrame".to_string(),
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
        "list_keys" => {
            let rows = KV.with_borrow(|kv| {
                let mut keys: Vec<&String> = kv.keys().collect();
                keys.sort();
                keys.into_iter().map(|k| json!({ "key": k })).collect::<Vec<_>>()
            });
            let rows_json = serde_json::to_string(&rows).map_err(|err| err.to_string())?;
            Ok(data_frame_output(
                vec![ColumnDef {
                    name: "key".to_string(),
                    alias: "Key".to_string(),
                    dtype: "string".to_string(),
                }],
                rows_json,
            ))
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

/// Wrap a schema + row-oriented JSON string in a `ToolOutput::DataFrameOutput`
/// with no optional metadata set.
fn data_frame_output(schema: Vec<ColumnDef>, rows: String) -> ToolOutput {
    ToolOutput::DataFrameOutput(DataFrameOutput {
        schema,
        rows,
        metadata: None,
        label: None,
        source: None,
        description: None,
        nickname: None,
        store: None,
    })
}

/// Reject any block kind other than `prefix-tracker`.
fn require_prefix_tracker(kind: &str) -> Result<(), String> {
    if kind == PREFIX_TRACKER_KIND {
        Ok(())
    } else {
        Err(format!("Unknown block kind: {kind}"))
    }
}

/// Parse a JSON string as an object, returning an error for any other value.
fn parse_json_object(input: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(input).map_err(|err| format!("invalid JSON: {err}"))?;
    if !value.is_object() {
        return Err("expected JSON object".to_string());
    }
    Ok(value)
}

/// Parse a `{"prefixes": [...]}` input object into a `Vec<String>`. An
/// absent or null `prefixes` field is treated as an empty list.
fn parse_prefix_list(input: &str) -> Result<Vec<String>, String> {
    let value = parse_json_object(input)?;
    match value.get("prefixes") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "prefixes must be an array of strings".to_string())
            })
            .collect(),
        Some(_) => Err("prefixes must be an array of strings".to_string()),
    }
}

/// Parse a block state JSON string, enforcing the `{"prefixes": [...]}` shape.
fn parse_state(state: &str) -> Result<Vec<String>, String> {
    let value = parse_json_object(state).map_err(|err| format!("invalid state: {err}"))?;
    let prefixes = value
        .get("prefixes")
        .ok_or_else(|| "invalid state: missing 'prefixes' field".to_string())?;
    let array = prefixes
        .as_array()
        .ok_or_else(|| "invalid state: 'prefixes' must be an array of strings".to_string())?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| "invalid state: 'prefixes' must be an array of strings".to_string())
        })
        .collect()
}

/// Serialize a prefix list back into the canonical `{"prefixes": [...]}` state JSON.
fn serialize_state(prefixes: &[String]) -> Result<String, String> {
    serde_json::to_string(&json!({ "prefixes": prefixes })).map_err(|err| err.to_string())
}

/// Build the query plan JSON for the current prefix set.
fn build_query_plan(prefixes: &[String]) -> Result<String, String> {
    serde_json::to_string(&json!({
        "resource": "kv-keys",
        "filter": { "key_starts_with_any": prefixes },
    }))
    .map_err(|err| err.to_string())
}

/// Stream the `kv-keys` host resource via a cursor, tallying UTF-8 character
/// frequencies across every row's `key` field. Returns a JSON object mapping
/// each character (as a one-char string) to its total count, sorted by char.
fn analyze_keys() -> Result<String, String> {
    // Query arg is opaque for Phase 5 — the host decides what to do with it.
    let cursor = Cursor::open("kv-keys", "").map_err(|err| format!("open cursor: {err}"))?;

    let mut freqs: BTreeMap<char, u32> = BTreeMap::new();
    loop {
        match cursor.next_batch(64) {
            Ok(Some(batch_json)) => {
                let rows: Vec<Value> =
                    serde_json::from_str(&batch_json).map_err(|err| format!("parse batch: {err}"))?;
                for row in rows {
                    if let Some(key) = row.get("key").and_then(Value::as_str) {
                        for ch in key.chars() {
                            *freqs.entry(ch).or_insert(0) += 1;
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(err) => return Err(format!("next batch: {err}")),
        }
    }

    // Serde serializes char map keys as one-char strings, giving us
    // `{"a": 3, "b": 1, ...}`. BTreeMap ordering makes the output
    // deterministic across runs.
    serde_json::to_string(&freqs).map_err(|err| format!("serialize freqs: {err}"))
}
