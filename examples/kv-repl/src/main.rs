//! Interactive REPL for the KV extension.
//!
//! Demonstrates loading an extension and sending commands interactively.
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
//!   quit                - Exit the REPL

use std::io::{self, BufRead, Write};

use futures::StreamExt;

use emporium::data::Response;
use emporium::{Command, Event, Extension};

/// Path to the KV extension, set by build.rs
const KV_EXTENSION_PATH: &str = env!("KV_EXTENSION_PATH");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Use command line arg if provided, otherwise use the built-in extension
    let path = std::env::args().nth(1).unwrap_or_else(|| KV_EXTENSION_PATH.to_string());

    println!("Loading extension from: {}", path);

    let config = serde_json::json!({});
    let mut ext = Extension::load(&path, config).await?;

    println!("Loaded: {} v{}", ext.id(), ext.version());
    println!("{}\n", ext.manifest().description);
    println!("Commands: get <key>, set <key> <value>, delete <key>, list, clear, stats, quit\n");

    // Take the event stream
    let mut events = ext.events().expect("events() can only be called once");

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

        let command = match cmd_str.as_str() {
            "quit" | "exit" | "q" => {
                println!("Goodbye!");
                break;
            }
            "get" => {
                if parts.len() < 2 {
                    println!("Usage: get <key>");
                    continue;
                }
                Command::ExecuteTool {
                    name: "get".to_string(),
                    params: serde_json::json!({ "key": parts[1] }),
                    correlation_id: None,
                }
            }
            "set" => {
                if parts.len() < 3 {
                    println!("Usage: set <key> <value>");
                    continue;
                }
                Command::ExecuteTool {
                    name: "set".to_string(),
                    params: serde_json::json!({ "key": parts[1], "value": parts[2] }),
                    correlation_id: None,
                }
            }
            "delete" | "del" => {
                if parts.len() < 2 {
                    println!("Usage: delete <key>");
                    continue;
                }
                Command::ExecuteTool {
                    name: "delete".to_string(),
                    params: serde_json::json!({ "key": parts[1] }),
                    correlation_id: None,
                }
            }
            "list" | "keys" | "all" => Command::ExecuteTool {
                name: "get_all".to_string(),
                params: serde_json::json!({}),
                correlation_id: None,
            },
            "clear" => Command::ExecuteTool {
                name: "clear".to_string(),
                params: serde_json::json!({}),
                correlation_id: None,
            },
            "stats" => Command::ExecuteTool {
                name: "stats".to_string(),
                params: serde_json::json!({}),
                correlation_id: None,
            },
            "tools" => Command::ListTools { correlation_id: None },
            "help" | "?" => {
                println!("Commands:");
                println!("  get <key>           - Get a value");
                println!("  set <key> <value>   - Set a value");
                println!("  delete <key>        - Delete a key");
                println!("  list                - List all keys");
                println!("  clear               - Clear all keys");
                println!("  stats               - Show store statistics");
                println!("  tools               - List available tools");
                println!("  quit                - Exit the REPL");
                continue;
            }
            _ => {
                println!("Unknown command: {}. Type 'help' for available commands.", cmd_str);
                continue;
            }
        };

        // Send the command
        ext.send(command)?;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), wait_for_response(&mut events)).await;

        match response {
            Ok(Some(resp)) => print_response(resp),
            Ok(None) => println!("Extension closed"),
            Err(_) => println!("Timeout waiting for response"),
        }
    }

    Ok(())
}

async fn wait_for_response(events: &mut (impl StreamExt<Item = Event> + Unpin)) -> Option<Response> {
    while let Some(event) = events.next().await {
        if let Event::Core(response) = event {
            return Some(response);
        }
    }
    None
}

fn print_response(response: Response) {
    match response {
        Response::ToolList { tools, .. } => {
            println!("Available tools:");
            for tool in tools {
                println!("  {} - {}", tool.name, tool.description);
            }
        }
        Response::ToolResult { name, result, .. } => match result {
            Ok(r) => {
                use emporium::data::core::tool::ToolResult;
                match r {
                    ToolResult::Text(text) => {
                        if text.content == "null" || text.content.is_empty() {
                            println!("OK");
                        } else {
                            println!("{}", text.content);
                        }
                    }
                    ToolResult::DataFrame(df) => {
                        println!("DataFrame with {} columns", df.schema.len());
                    }
                }
            }
            Err(e) => println!("Error from '{}': {}", name, e),
        },
        Response::Error { message, .. } => {
            println!("Error: {}", message);
        }
        Response::Metadata { name, version, .. } => {
            println!("Extension: {} v{}", name, version);
        }
        Response::ToolDetails { tool_info, .. } => {
            println!("{}: {}", tool_info.name, tool_info.description);
        }
    }
}
