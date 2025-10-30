use emporium::*;
use futures::StreamExt;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load API key from .env
    dotenvy::dotenv().ok();
    let api_key = std::env::var("ALPHAVANTAGE_API_KEY")?;

    // Load the alphav extension
    let extension_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("marketplace/build/xt-alphav/extension.wasm");

    let config = json!({
        "api_key": api_key,
        "base_url": "https://www.alphavantage.co"
    })
    .to_string();

    let alphav = wasm::load("alphav".to_string(), config, extension_path).await?;

    // Create the sipper
    let mut sipper = Box::pin(alphav.into_sipper());
    let mut sender = None;

    // Process initial events until we get Connected
    while let Some(response) = sipper.next().await {
        match response {
            Event::Connected(msg_tx) => {
                println!("✓ Extension connected");
                sender = Some(msg_tx);
                break; // Got sender, can proceed
            }
            Event::Metadata { id, name, version, .. } => {
                println!("✓ Loaded: {} {} v{}", id, name, version);
            }
            _ => {}
        }
    }

    if let Some(tx) = sender {
        // Example 1: List available tools
        println!("\n→ Listing available tools...");
        tx.unbounded_send(Command::ListTools)?;

        if let Some(response) = sipper.next().await {
            match response {
                Event::ToolList(tools) => {
                    println!("← Available tools:");
                    for tool in tools {
                        println!("  - {}: {}", tool.id, tool.description);
                    }
                }
                Event::Error(err) => eprintln!("✗ Error: {}", err),
                _ => {}
            }
        }

        // Example 2: Get daily time series for AAPL
        println!("\n→ Getting daily time series for AAPL...");
        let command = Command::ExecuteTool {
            tool_id: "time_series_daily".to_string(),
            params: json!({
                "symbol": "AAPL",
                "outputsize": "compact"
            }),
        };
        tx.unbounded_send(command)?;

        if let Some(response) = sipper.next().await {
            match response {
                Event::ToolResult { tool_id, result } => {
                    println!("← Response from '{}' tool:", tool_id);
                    // Pretty print the JSON
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                Event::Data(json_str) => {
                    println!("← Response (legacy format):");
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        println!("{}", serde_json::to_string_pretty(&json_val)?);
                    } else {
                        println!("{}", json_str);
                    }
                }
                Event::Error(err) => {
                    eprintln!("✗ Error: {}", err);
                }
                _ => {}
            }
        }

        // Example 3: Get intraday data (5-minute intervals)
        println!("\n→ Getting intraday data for AAPL (5-min intervals)...");
        let command = Command::ExecuteTool {
            tool_id: "time_series_intraday".to_string(),
            params: json!({
                "symbol": "AAPL",
                "interval": "5min",
                "outputsize": "compact"
            }),
        };
        tx.unbounded_send(command)?;

        if let Some(response) = sipper.next().await {
            match response {
                Event::ToolResult { tool_id, result } => {
                    println!("← Response from '{}' tool:", tool_id);
                    // Show just a summary since intraday data can be large
                    if let Some(meta) = result.get("Meta Data") {
                        println!("  Meta Data: {}", serde_json::to_string_pretty(meta)?);
                    }
                    if let Some(series_key) = result.as_object()
                        .and_then(|obj| obj.keys().find(|k| k.contains("Time Series"))) {
                        if let Some(series) = result.get(series_key).and_then(|v| v.as_object()) {
                            println!("  Time series entries: {}", series.len());
                            // Show just the first entry
                            if let Some((timestamp, data)) = series.iter().next() {
                                println!("  First entry: {} -> {}", timestamp, 
                                    serde_json::to_string_pretty(data)?);
                            }
                        }
                    }
                }
                Event::Data(json_str) => {
                    println!("← Response (legacy format):");
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        // Show summary for large responses
                        if let Some(obj) = json_val.as_object() {
                            println!("  Keys: {:?}", obj.keys().collect::<Vec<_>>());
                        }
                    }
                }
                Event::Error(err) => {
                    eprintln!("✗ Error: {}", err);
                }
                _ => {}
            }
        }

        // Example 4: Try the search endpoint (currently not implemented)
        println!("\n→ Testing symbol search...");
        let command = Command::ExecuteTool {
            tool_id: "search".to_string(),
            params: json!({
                "keywords": "Apple"
            }),
        };
        tx.unbounded_send(command)?;

        if let Some(response) = sipper.next().await {
            match response {
                Event::ToolResult { tool_id, result } => {
                    println!("← Response from '{}' tool:", tool_id);
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                Event::Data(json_str) => {
                    println!("← Response (legacy format):");
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        println!("{}", serde_json::to_string_pretty(&json_val)?);
                    }
                }
                Event::Error(err) => {
                    eprintln!("✗ Error: {}", err);
                }
                _ => {}
            }
        }

        // Close the sender to cleanly shut down
        drop(tx);
    } else {
        eprintln!("✗ Failed to get sender from extension");
    }

    // Drain remaining events
    while let Some(_) = sipper.next().await {}
    sipper.await;

    Ok(())
}
