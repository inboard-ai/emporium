//! End-to-end integration tests for the kv-repl extension.
//!
//! These tests load the packaged kv-extension via `Extension::load` and
//! exercise the typed API (`list_tools`, `execute_tool`) plus the
//! host-events broadcast channel.

use std::time::Duration;

use emporium::Extension;
use emporium_core::{event, tool};
use serde_json::json;

/// Path to the packaged kv-extension, baked in by kv-repl's build.rs.
const KV_EXTENSION_PATH: &str = env!("KV_EXTENSION_PATH");

#[tokio::test(flavor = "current_thread")]
async fn loads_lists_and_executes_kv_tools() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let tools = ext.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 6, "kv-extension should advertise 6 tools");
    let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
    for expected in ["get", "set", "delete", "get_all", "clear", "stats"] {
        assert!(ids.contains(&expected), "missing tool: {expected} (got {ids:?})");
    }

    ext.execute_tool("set", json!({"key": "hello", "value": "world"}))
        .await
        .expect("set should succeed");

    let got = ext.execute_tool("get", json!({"key": "hello"})).await.expect("get");
    match got {
        tool::Output::Text(text) => assert_eq!(text.content, "world"),
        other => panic!("expected Text output, got {other:?}"),
    }

    let stats = ext.execute_tool("stats", json!({})).await.expect("stats");
    match stats {
        tool::Output::Text(text) => assert_eq!(text.content, "1 entry"),
        other => panic!("expected Text output, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn progress_event_fanned_out_to_subscribers() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    // Subscribe *before* the tool call so we don't miss the event.
    let mut events = ext.events();

    ext.execute_tool("stats", json!({}))
        .await
        .expect("stats should succeed");

    let received = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("event arrived within timeout")
        .expect("event channel still open");

    match received {
        event::Event::Progress(progress) => {
            assert!(
                progress.message.contains("stats"),
                "progress message should mention 'stats', got: {:?}",
                progress.message
            );
        }
        other => panic!("expected Progress event, got {other:?}"),
    }
}
