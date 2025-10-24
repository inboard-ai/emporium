use emporium::*;
use serde_json::json;
use sipper::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load API key from .env
    dotenvy::dotenv().ok();
    let api_key = std::env::var("POLYGON_API_KEY")?;

    // Load the polygon extension
    let extension_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("marketplace/build/xt-polygon/xt_polygon.wasm");

    let polygon = wasm::load("polygon".to_string(), extension_path)
        .await?
        .with_config(json!({ "api_key": api_key }).to_string());

    let (sipper, msg_tx) = polygon.into_sipper();

    let response_task = tokio::spawn(async move {
        let mut sipper = Box::pin(sipper);
        let mut responses = Vec::new();
        while let Some(resp) = sipper.next().await {
            responses.push(resp);
        }
        responses
    });

    // Send command to get related tickers for AAPL
    let command = json!({
        "method": "related_tickers",
        "ticker": "AAPL"
    });
    msg_tx.unbounded_send(Message(command.to_string()))?;

    // Close sender
    drop(msg_tx);

    // Wait for response
    let responses = response_task.await?;

    // Print the result (skip metadata and ready events)
    for resp in responses.iter().skip(2) {
        println!("{}", resp.0);
    }

    Ok(())
}
