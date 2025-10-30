use serde_json::{Value, json};
use sipper::{Sipper, StreamExt};

use emporium::*;

/// Collect all responses from the extension
async fn collect_responses<S>(mut sipper: S) -> Vec<Value>
where
    S: Sipper<(), Event> + Unpin,
{
    let mut responses = Vec::new();
    // while let Some(resp) = sipper.next().await {
    //     let json: Value = serde_json::from_str(&resp.0).unwrap_or_else(|_| json!({"error": "Invalid JSON"}));
    //     responses.push(json);
    // }
    // eprintln!("Collected {} responses: {:#?}", responses.len(), responses);
    responses
}

/// Run test commands against the KV store
fn run_tests(tx: &futures::channel::mpsc::UnboundedSender<Command>) {
    let send = |cmd: Value| {
        let msg_str = cmd.to_string();
        // eprintln!("Sending message: {}", msg_str);
        tx.unbounded_send(Command(msg_str)).unwrap();
    };

    send(json!({"method": "set", "key": "name", "value": "Alice"}));
    send(json!({"method": "get", "key": "name"}));
    send(json!({"method": "set", "key": "age", "value": "30"}));
    send(json!({"method": "get", "key": "age"}));
    send(json!({"method": "get", "key": "missing"}));
    send(json!({"method": "delete", "key": "name"}));
    send(json!({"method": "get", "key": "name"}));
    send(json!({"method": "delete", "key": "missing"}));
    send(json!({"method": "unknown"}));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // // Load KV extension
    // let extension_dir = format!("{}/marketplace/build", std::env::var("CARGO_MANIFEST_DIR")?);
    // let mut list = emporium::list(&extension_dir).pin();

    // let mut kv_extension = None;
    // while let Some((path, manifest)) = list.next().await {
    //     if manifest.id == "kv" {
    //         kv_extension = Some(emporium::load(manifest.id, "".to_string(), path).await?);
    //         break;
    //     }
    // }

    // let (sipper, msg_tx) = kv_extension.ok_or("KV extension not found")?.into_sipper();

    // let response_task = tokio::spawn(async move { collect_responses(Box::pin(sipper)).await });

    // // Give the extension time to start up and emit metadata + ready events
    // tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // run_tests(&msg_tx);
    // drop(msg_tx);

    // let responses = response_task.await?;
    // // skip metadata and ready events
    // let results: Vec<_> = responses.iter().skip(2).collect();

    // println!("\nVerifying KV store operations...");

    // // SET name -> "OK"
    // assert_eq!(
    //     results[0],
    //     &Value::Null,
    //     "SET name should return null (OK serializes as unit)"
    // );
    // println!("✓ SET name = Alice");

    // // GET name -> "Alice"
    // assert_eq!(results[1], &Value::String("Alice".to_string()));
    // println!("✓ GET name = Alice");

    // // SET age -> "OK"
    // assert_eq!(
    //     results[2],
    //     &Value::Null,
    //     "SET age should return null (OK serializes as unit)"
    // );
    // println!("✓ SET age = 30");

    // // GET age -> "30"
    // assert_eq!(results[3], &Value::String("30".to_string()));
    // println!("✓ GET age = 30");

    // // GET missing -> null
    // assert_eq!(results[4], &Value::Null);
    // println!("✓ GET missing key returns null");

    // // DELETE name -> 1
    // assert_eq!(results[5], &Value::Number(1.into()));
    // println!("✓ DELETE name returned 1");

    // // GET name -> null (deleted)
    // assert_eq!(results[6], &Value::Null);
    // println!("✓ GET deleted key returns null");

    // // DELETE missing -> 0
    // assert_eq!(results[7], &Value::Number(0.into()));
    // println!("✓ DELETE missing key returned 0");

    // // Unknown method -> error
    // assert!(results[8]["error"].as_str().unwrap().contains("unknown variant"));
    // println!("✓ Unknown method returns error");

    // println!("\n✅ All KV store tests passed!");
    Ok(())
}
