//! Interactive REPL for the KV extension.
//!
//! Demonstrates loading an extension and sending typed calls interactively.
//! The KV extension is built automatically as part of the build process.
//!
//! Usage:
//!   cargo run -p kv-repl
//!
//! Commands:
//!   get <key>           - Get a value
//!   set <key> <value>   - Set a value
//!   delete <key>        - Delete a key
//!   list                - List all keys
//!   clear               - Clear all keys
//!   stats               - Show store statistics
//!   tools               - List available tools
//!   view                - Print the extension's rendered view
//!   help                - Print this help message
//!   quit                - Exit the REPL

use std::io::{self, BufRead, Write};
use std::time::Duration;

use emporium::Extension;
use emporium_core::tool;
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
    println!("Commands: get <key>, set <key> <value>, delete <key>, list, clear, stats, tools, view, help, quit\n");

    // Drain the host-events broadcast stream in the background so users can
    // see progress events emitted by the extension.
    let events = ext.events();
    tokio::spawn(drain_events(events));

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
            "help" | "?" => {
                println!("Commands:");
                println!("  get <key>           - Get a value");
                println!("  set <key> <value>   - Set a value");
                println!("  delete <key>        - Delete a key");
                println!("  list                - List all keys");
                println!("  clear               - Clear all keys");
                println!("  stats               - Show store statistics");
                println!("  tools               - List available tools");
                println!("  view                - Print the extension's rendered view");
                println!("  quit                - Exit the REPL");
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
