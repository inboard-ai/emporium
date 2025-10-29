use emporium::*;
use futures::StreamExt;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load API key from .env
    dotenvy::dotenv().ok();
    let api_key = std::env::var("POLYGON_API_KEY")?;

    // Load the polygon extension
    let extension_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("marketplace/build/xt-polygon/extension.wasm");

    let config = json!({
        "api_key": api_key,
        "base_url": "https://api.polygon.io"
    })
    .to_string();

    let polygon = wasm::load("polygon".to_string(), config, extension_path).await?;

    // Create the sipper
    let mut sipper = Box::pin(polygon.into_sipper());
    let mut sender = None;

    // Process initial events until we get Connected
    while let Some(response) = sipper.next().await {
        match response {
            Response::Connected(msg_tx) => {
                println!("✓ Extension connected");
                sender = Some(msg_tx);
                break; // Got sender, can proceed
            }
            Response::Metadata { id, name, version, .. } => {
                println!("✓ Loaded: {} {} v{}", id, name, version);
            }
            _ => {}
        }
    }

    // Send command to get related tickers
    if let Some(tx) = sender {
        let command = json!({
            "method": "related_tickers",
            "ticker": "AAPL"
        });

        println!("→ Sending command: {}", command);
        tx.unbounded_send(Command(command.to_string()))?;

        // Get the response
        if let Some(response) = sipper.next().await {
            match response {
                Response::Data(json_str) => {
                    println!("← Response received:");

                    // Pretty print the JSON
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        println!("{}", serde_json::to_string_pretty(&json_val)?);
                    } else {
                        println!("{}", json_str);
                    }
                }
                Response::Error(err) => {
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
