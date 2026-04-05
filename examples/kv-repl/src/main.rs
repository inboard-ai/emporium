//! Interactive REPL for the KV extension.
//!
//! Demonstrates loading an extension and sending typed calls interactively.
//! The KV extension is built automatically as part of the build process.
//!
//! Usage:
//!   cargo run -p kv-repl
//!
//! Commands:
//!   get <key>                     - Get a value
//!   set <key> <value>             - Set a value
//!   delete <key>                  - Delete a key
//!   list                          - List all keys
//!   clear                         - Clear all keys
//!   stats                         - Show store statistics
//!   tools                         - List available tools
//!   view                          - Print the extension's rendered view
//!   block-types                   - List block kinds advertised by the extension
//!   block-create <prefix>...      - Create a prefix-tracker block
//!   block-add <prefix>            - Add a prefix to the current block state
//!   block-rename <old> <new>      - Rename a prefix and re-query matching keys
//!   block-query                   - Run the current block's query plan
//!   formulas                      - List formulas advertised by the extension
//!   formula <name> [args...]      - Evaluate a formula with positional args
//!   help                          - Print this help message
//!   quit                          - Exit the REPL

use std::io::{self, BufRead, Write};
use std::time::Duration;

use emporium::{Extension, block, formula};
use emporium_core::{plan, tool};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

/// Path to the KV extension, set by build.rs.
const KV_EXTENSION_PATH: &str = env!("KV_EXTENSION_PATH");

/// Timeout applied to every tool call round-trip.
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Use command line arg if provided, otherwise use the built-in extension.
    let path = std::env::args().nth(1).unwrap_or_else(|| KV_EXTENSION_PATH.to_string());

    println!("Loading extension from: {path}");

    let ext = Extension::load(&path, json!({})).await?;

    println!("Loaded: {} v{}", ext.id(), ext.version());
    println!("{}\n", ext.description());
    println!(
        "Commands: get <key>, set <key> <value>, delete <key>, list, clear, stats, tools, view,\n\
         block-types, block-create <prefix>..., block-add <prefix>, block-rename <old> <new>,\n\
         block-query, formulas, formula <name> [args...], help, quit\n"
    );

    // Bind the block provider once. The kv-extension advertises the
    // `block-extension` world, so this should always be `Some` at startup.
    let block_provider = ext.block().ok_or("extension does not export a block provider")?;

    // Bind the formula provider once. The kv-extension advertises the
    // `rich-extension` world, so this should always be `Some` at startup.
    let formula_provider = ext.formula().ok_or("extension does not export a formula provider")?;

    // Drain the host-events broadcast stream in the background so users can
    // see progress events emitted by the extension.
    let events = ext.events();
    tokio::spawn(drain_events(events));

    // Block-provider state tracked across block-* commands. `None` until the
    // user runs `block-create`.
    let mut block_state: Option<String> = None;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("kv> ");
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        let cmd_str = parts[0].to_lowercase();

        match cmd_str.as_str() {
            "quit" | "exit" | "q" => {
                println!("Goodbye!");
                break;
            }
            "get" => {
                if parts.len() < 2 {
                    println!("Usage: get <key>");
                    continue;
                }
                dispatch_tool(&ext, "get", json!({ "key": parts[1] })).await;
            }
            "set" => {
                if parts.len() < 3 {
                    println!("Usage: set <key> <value>");
                    continue;
                }
                dispatch_tool(&ext, "set", json!({ "key": parts[1], "value": parts[2] })).await;
            }
            "delete" | "del" => {
                if parts.len() < 2 {
                    println!("Usage: delete <key>");
                    continue;
                }
                dispatch_tool(&ext, "delete", json!({ "key": parts[1] })).await;
            }
            "list" | "keys" | "all" => dispatch_tool(&ext, "get_all", json!({})).await,
            "clear" => dispatch_tool(&ext, "clear", json!({})).await,
            "stats" => dispatch_tool(&ext, "stats", json!({})).await,
            "tools" => match ext.list_tools().await {
                Ok(tools) => {
                    println!("Available tools:");
                    for info in tools {
                        println!("  {} - {}", info.name, info.description);
                    }
                }
                Err(err) => println!("Error listing tools: {err}"),
            },
            "view" => match ext.view().await {
                Ok(rendered) => println!("{rendered}"),
                Err(err) => println!("Error rendering view: {err}"),
            },
            "block-types" => list_block_types(&block_provider).await,
            "block-create" => {
                // Collect all whitespace-separated prefixes after the command.
                let prefixes: Vec<String> = line.split_whitespace().skip(1).map(str::to_string).collect();
                block_create(&block_provider, &prefixes, &mut block_state).await;
            }
            "block-add" => {
                if parts.len() < 2 {
                    println!("Usage: block-add <prefix>");
                    continue;
                }
                block_add(&block_provider, parts[1], &mut block_state).await;
            }
            "block-rename" => {
                // parts[2] may contain trailing whitespace if the user typed
                // more than three tokens; take only the first whitespace-
                // separated chunk as `new`.
                if parts.len() < 3 {
                    println!("Usage: block-rename <old> <new>");
                    continue;
                }
                let new = parts[2].split_whitespace().next().unwrap_or("");
                if new.is_empty() {
                    println!("Usage: block-rename <old> <new>");
                    continue;
                }
                block_rename(&ext, &block_provider, parts[1], new, &mut block_state).await;
            }
            "block-query" => block_query(&ext, &block_provider, &block_state).await,
            "formulas" => list_formulas(&formula_provider).await,
            "formula" => {
                // Collect the formula name plus any positional arguments, all
                // whitespace-separated after the command.
                let mut tokens = line.split_whitespace().skip(1);
                let Some(name) = tokens.next() else {
                    println!("Usage: formula <name> [args...]");
                    continue;
                };
                let args: Vec<String> = tokens.map(str::to_string).collect();
                evaluate_formula(&formula_provider, name, &args).await;
            }
            "help" | "?" => {
                println!("Commands:");
                println!("  get <key>                     - Get a value");
                println!("  set <key> <value>             - Set a value");
                println!("  delete <key>                  - Delete a key");
                println!("  list                          - List all keys");
                println!("  clear                         - Clear all keys");
                println!("  stats                         - Show store statistics");
                println!("  tools                         - List available tools");
                println!("  view                          - Print the extension's rendered view");
                println!("  block-types                   - List block kinds advertised by the extension");
                println!("  block-create <prefix>...      - Create a prefix-tracker block");
                println!("  block-add <prefix>            - Add a prefix to the current block state");
                println!("  block-rename <old> <new>      - Rename a prefix and re-query matching keys");
                println!("  block-query                   - Run the current block's query plan");
                println!("  formulas                      - List formulas advertised by the extension");
                println!("  formula <name> [args...]      - Evaluate a formula with positional args");
                println!("  quit                          - Exit the REPL");
            }
            _ => {
                println!("Unknown command: {cmd_str}. Type 'help' for available commands.");
            }
        }
    }

    Ok(())
}

/// Execute a tool with the given parameters, print the output, and keep
/// the REPL running on errors.
async fn dispatch_tool(ext: &Extension, name: &str, params: serde_json::Value) {
    let call = tokio::time::timeout(TOOL_TIMEOUT, ext.execute_tool(name, params)).await;
    match call {
        Ok(Ok(output)) => print_output(output),
        Ok(Err(err)) => println!("Error from '{name}': {err}"),
        Err(_) => println!("Timeout waiting for '{name}' response"),
    }
}

/// Pretty-print a tool output — collapses null/empty text to "OK" to match the
/// REPL's prior behaviour.
fn print_output(output: tool::Output) {
    match output {
        tool::Output::Text(text) => {
            if text.content == "null" || text.content.is_empty() {
                println!("OK");
            } else {
                println!("{}", text.content);
            }
        }
        tool::Output::DataFrame(df) => {
            println!("DataFrame with {} columns", df.schema.len());
        }
        other => {
            let _ = other;
            println!("(unrecognized output variant)");
        }
    }
}

/// Background task that prints host-events as they arrive. Treats
/// `RecvError::Lagged` as a re-sync opportunity and keeps going.
async fn drain_events(mut events: tokio::sync::broadcast::Receiver<emporium_core::event::Event>) {
    loop {
        match events.recv().await {
            Ok(event) => {
                let description = describe_event(&event);
                println!("[event] {description}");
            }
            Err(RecvError::Lagged(n)) => {
                println!("[event] (lagged; dropped {n} events)");
            }
            Err(RecvError::Closed) => return,
        }
    }
}

/// One-line description of a host-events notification for REPL display.
fn describe_event(event: &emporium_core::event::Event) -> String {
    use emporium_core::event::{Event, Invalidate};
    match event {
        Event::Progress(p) => match p.percent {
            Some(pct) => format!("progress {pct}%: {}", p.message),
            None => format!("progress: {}", p.message),
        },
        Event::ToolsChanged => "tools-changed".to_string(),
        Event::Invalidate(Invalidate::Resource(id)) => format!("invalidate resource: {id}"),
        Event::Invalidate(Invalidate::Tool(name)) => format!("invalidate tool: {name}"),
        Event::Invalidate(Invalidate::All) => "invalidate all".to_string(),
        other => {
            let _ = other;
            "(unrecognized event)".to_string()
        }
    }
}

/// Kind identifier for the block type the kv-extension advertises.
const PREFIX_TRACKER_KIND: &str = "prefix-tracker";

/// Message printed when a block-* command runs before `block-create`.
const BLOCK_MISSING_HINT: &str = "No block created. Run 'block-create <prefix> ...' first.";

/// List and print the block types the extension advertises.
async fn list_block_types(provider: &block::Provider) {
    match provider.types().await {
        Ok(types) if types.is_empty() => println!("(no block types)"),
        Ok(types) => {
            println!("Block types:");
            for info in types {
                println!("  {}: {} - {}", info.kind, info.name, info.description);
            }
        }
        Err(err) => println!("Error listing block types: {err}"),
    }
}

/// Create a prefix-tracker block from the supplied prefix list and stash
/// the returned state JSON in `block_state`.
async fn block_create(provider: &block::Provider, prefixes: &[String], block_state: &mut Option<String>) {
    if prefixes.is_empty() {
        println!("Warning: creating block with empty prefix list (no keys will match).");
    }
    let input = json!({ "prefixes": prefixes });
    match provider.create(PREFIX_TRACKER_KIND, input).await {
        Ok(state) => {
            println!("Block created. State: {state}");
            *block_state = Some(state);
        }
        Err(err) => println!("Error creating block: {err}"),
    }
}

/// Run the `add_prefix` block operation, updating `block_state` on success.
async fn block_add(provider: &block::Provider, prefix: &str, block_state: &mut Option<String>) {
    let Some(state) = block_state.as_deref() else {
        println!("{BLOCK_MISSING_HINT}");
        return;
    };
    let input = json!({ "prefix": prefix });
    match provider.plan(PREFIX_TRACKER_KIND, state, "add_prefix", input).await {
        Ok(plan::Outcome::StateUpdate(new_state)) => {
            println!("Updated state: {new_state}");
            *block_state = Some(new_state);
        }
        Ok(other) => {
            let _ = other;
            println!("Warning: expected StateUpdate from add_prefix, got a different outcome.");
        }
        Err(err) => println!("Error running add_prefix: {err}"),
    }
}

/// Run the `rename_prefix` block operation, update state, then execute the
/// follow-up query plan against the KV store.
async fn block_rename(
    ext: &Extension,
    provider: &block::Provider,
    old: &str,
    new: &str,
    block_state: &mut Option<String>,
) {
    let Some(state) = block_state.as_deref() else {
        println!("{BLOCK_MISSING_HINT}");
        return;
    };
    let input = json!({ "old": old, "new": new });
    match provider.plan(PREFIX_TRACKER_KIND, state, "rename_prefix", input).await {
        Ok(plan::Outcome::StateUpdateWithQuery(new_state, plan_json)) => {
            println!("Updated state: {new_state}");
            *block_state = Some(new_state);
            println!("Rename plan executed:");
            run_toy_plan(ext, &plan_json).await;
        }
        Ok(other) => {
            let _ = other;
            println!("Warning: expected StateUpdateWithQuery from rename_prefix, got a different outcome.");
        }
        Err(err) => println!("Error running rename_prefix: {err}"),
    }
}

/// Run the `query` block operation and execute the returned plan.
async fn block_query(ext: &Extension, provider: &block::Provider, block_state: &Option<String>) {
    let Some(state) = block_state.as_deref() else {
        println!("{BLOCK_MISSING_HINT}");
        return;
    };
    match provider.plan(PREFIX_TRACKER_KIND, state, "query", json!({})).await {
        Ok(plan::Outcome::Query(plan_json)) => {
            println!("Query plan matched:");
            run_toy_plan(ext, &plan_json).await;
        }
        Ok(other) => {
            let _ = other;
            println!("Warning: expected Query from query, got a different outcome.");
        }
        Err(err) => println!("Error running query: {err}"),
    }
}

/// Toy plan executor: resolves `kv-keys` by calling `list_keys` and filters
/// rows whose `key` field starts with any prefix in
/// `filter.key_starts_with_any`. This is a demonstration, not a general
/// plan engine — it only understands the one plan shape kv-repl emits.
async fn run_toy_plan(ext: &Extension, plan_json: &str) {
    let plan: serde_json::Value = match serde_json::from_str(plan_json) {
        Ok(value) => value,
        Err(err) => {
            println!("Error parsing plan JSON: {err}");
            return;
        }
    };

    let resource = plan.get("resource").and_then(|v| v.as_str()).unwrap_or("");
    if resource != "kv-keys" {
        println!("Unknown resource: {resource}");
        return;
    }

    let output = match ext.execute_tool("list_keys", json!({})).await {
        Ok(output) => output,
        Err(err) => {
            println!("Error running list_keys: {err}");
            return;
        }
    };

    let df = match output {
        tool::Output::DataFrame(df) => df,
        other => {
            let _ = other;
            println!("Unexpected list_keys output variant");
            return;
        }
    };

    let rows = match df.data.as_array() {
        Some(rows) => rows,
        None => {
            println!("Unexpected list_keys data shape (expected JSON array)");
            return;
        }
    };
    let keys: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get("key").and_then(|v| v.as_str()))
        .collect();

    let prefixes = match plan.get("filter").and_then(|f| f.get("key_starts_with_any")) {
        Some(serde_json::Value::Array(items)) => {
            let parsed: Vec<&str> = items.iter().filter_map(|item| item.as_str()).collect();
            if parsed.len() != items.len() {
                println!("Warning: non-string entries in key_starts_with_any were ignored.");
            }
            parsed
        }
        _ => {
            println!("No filter applied. Plan: {plan_json}");
            return;
        }
    };

    let matches: Vec<&str> = keys
        .into_iter()
        .filter(|key| prefixes.iter().any(|prefix| key.starts_with(prefix)))
        .collect();

    if matches.is_empty() {
        println!("(no matching keys)");
    } else {
        for key in matches {
            println!("{key}");
        }
    }
}

/// List the formulas the extension advertises, printing each as
/// `"{name}: {description}"`.
async fn list_formulas(provider: &formula::Provider) {
    match provider.defs().await {
        Ok(defs) if defs.is_empty() => println!("(no formulas)"),
        Ok(defs) => {
            println!("Formulas:");
            for def in defs {
                println!("  {}: {}", def.name, def.description);
            }
        }
        Err(err) => println!("Error listing formulas: {err}"),
    }
}

/// Evaluate a named formula with positional string arguments. The returned
/// JSON-encoded result is printed literally (no quote stripping).
async fn evaluate_formula(provider: &formula::Provider, name: &str, args: &[String]) {
    let args_value = json!(args);
    match provider.evaluate(name, args_value).await {
        Ok(result) => println!("{result}"),
        Err(err) => println!("Error evaluating formula: {err}"),
    }
}
