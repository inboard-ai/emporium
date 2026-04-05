//! End-to-end integration tests for the kv-repl extension.
//!
//! These tests load the packaged kv-extension via `Extension::load` and
//! exercise the typed API (`list_tools`, `execute_tool`) plus the
//! host-events broadcast channel.

use std::time::Duration;

use emporium::Extension;
use emporium_core::{event, plan, tool};
use serde_json::json;

/// Path to the packaged kv-extension, baked in by kv-repl's build.rs.
const KV_EXTENSION_PATH: &str = env!("KV_EXTENSION_PATH");

#[tokio::test(flavor = "current_thread")]
async fn loads_lists_and_executes_kv_tools() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let tools = ext.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 7, "kv-extension should advertise 7 tools");
    let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
    for expected in ["get", "set", "delete", "get_all", "clear", "stats", "list_keys"] {
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

#[tokio::test(flavor = "current_thread")]
async fn block_provider_returns_prefix_tracker_type() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let provider = ext.block().expect("block provider should be available");
    let types = provider.types().await.expect("list block types");

    assert_eq!(types.len(), 1, "expected exactly one block type, got {types:?}");
    assert_eq!(types[0].kind, "prefix-tracker");
}

#[tokio::test(flavor = "current_thread")]
async fn block_create_and_plan_query_round_trip() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    // Seed a few keys so the plan, were it executed, would match real rows.
    ext.execute_tool("set", json!({"key": "alpha_1", "value": "v1"}))
        .await
        .expect("set alpha_1");
    ext.execute_tool("set", json!({"key": "alpha_2", "value": "v2"}))
        .await
        .expect("set alpha_2");
    ext.execute_tool("set", json!({"key": "beta_1", "value": "v3"}))
        .await
        .expect("set beta_1");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["alpha_"]}))
        .await
        .expect("create prefix-tracker block");
    assert!(
        state.contains("alpha_"),
        "state JSON should mention 'alpha_' prefix, got: {state}"
    );

    let outcome = provider
        .plan("prefix-tracker", &state, "query", json!({}))
        .await
        .expect("plan query");
    let plan_json = match outcome {
        plan::Outcome::Query(plan_json) => plan_json,
        other => panic!("expected Outcome::Query, got {other:?}"),
    };

    let plan: serde_json::Value = serde_json::from_str(&plan_json).expect("parse plan JSON");
    assert_eq!(plan.get("resource").and_then(|v| v.as_str()), Some("kv-keys"));
    let prefixes = plan
        .get("filter")
        .and_then(|f| f.get("key_starts_with_any"))
        .and_then(|v| v.as_array())
        .expect("filter.key_starts_with_any array");
    let prefix_strs: Vec<&str> = prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(prefix_strs, vec!["alpha_"]);
}

#[tokio::test(flavor = "current_thread")]
async fn block_add_prefix_updates_state() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["a"]}))
        .await
        .expect("create block");

    let outcome = provider
        .plan("prefix-tracker", &state, "add_prefix", json!({"prefix": "b"}))
        .await
        .expect("add_prefix b");
    let new_state = match outcome {
        plan::Outcome::StateUpdate(new_state) => new_state,
        other => panic!("expected StateUpdate, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&new_state).expect("parse state");
    let prefixes = parsed
        .get("prefixes")
        .and_then(|v| v.as_array())
        .expect("prefixes array");
    let prefix_strs: Vec<&str> = prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(prefix_strs, vec!["a", "b"]);

    // Re-adding "a" is a no-op: the prefix list stays `["a", "b"]`.
    let outcome = provider
        .plan("prefix-tracker", &new_state, "add_prefix", json!({"prefix": "a"}))
        .await
        .expect("add_prefix a (idempotent)");
    let new_state_2 = match outcome {
        plan::Outcome::StateUpdate(new_state) => new_state,
        other => panic!("expected StateUpdate, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&new_state_2).expect("parse state");
    let prefixes = parsed
        .get("prefixes")
        .and_then(|v| v.as_array())
        .expect("prefixes array");
    let prefix_strs: Vec<&str> = prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(prefix_strs, vec!["a", "b"], "add_prefix should be idempotent");
}

#[tokio::test(flavor = "current_thread")]
async fn block_rename_prefix_returns_state_update_with_query() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["foo", "bar"]}))
        .await
        .expect("create block");

    let outcome = provider
        .plan(
            "prefix-tracker",
            &state,
            "rename_prefix",
            json!({"old": "foo", "new": "baz"}),
        )
        .await
        .expect("rename_prefix foo -> baz");
    let (new_state, plan_json) = match outcome {
        plan::Outcome::StateUpdateWithQuery(new_state, plan_json) => (new_state, plan_json),
        other => panic!("expected StateUpdateWithQuery, got {other:?}"),
    };

    let parsed_state: serde_json::Value = serde_json::from_str(&new_state).expect("parse state");
    let prefixes = parsed_state
        .get("prefixes")
        .and_then(|v| v.as_array())
        .expect("prefixes array");
    let prefix_strs: Vec<&str> = prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        prefix_strs,
        vec!["baz", "bar"],
        "in-place rename should preserve ordering"
    );

    let plan: serde_json::Value = serde_json::from_str(&plan_json).expect("parse plan JSON");
    let filter_prefixes = plan
        .get("filter")
        .and_then(|f| f.get("key_starts_with_any"))
        .and_then(|v| v.as_array())
        .expect("filter.key_starts_with_any array");
    let filter_strs: Vec<&str> = filter_prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(filter_strs, vec!["baz", "bar"]);
}
