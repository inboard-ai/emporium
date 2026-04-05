//! End-to-end integration tests for the kv-repl extension.
//!
//! These tests load the packaged kv-extension via `Extension::load` and
//! exercise the typed API (`list_tools`, `execute_tool`) plus the
//! host-events broadcast channel.

use std::time::Duration;

use emporium::host_data::DataRegistry;
use emporium::{Error, Extension};
use emporium_core::{column, event, plan, tool};
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

#[tokio::test(flavor = "current_thread")]
async fn formula_provider_lists_three_formulas() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let provider = ext.formula().expect("formula provider should be available");
    let defs = provider.defs().await.expect("list formula defs");

    assert_eq!(defs.len(), 3, "expected exactly 3 formulas, got {defs:?}");
    let mut names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["KV_COUNT", "KV_EXISTS", "KV_GET"]);
}

#[tokio::test(flavor = "current_thread")]
async fn formula_kv_count_returns_number() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let provider = ext.formula().expect("formula provider should be available");

    let result = provider
        .evaluate("KV_COUNT", json!([]))
        .await
        .expect("evaluate KV_COUNT on empty store");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_COUNT result");
    assert_eq!(parsed, json!(0));

    ext.execute_tool("set", json!({"key": "a", "value": "1"}))
        .await
        .expect("set a");
    ext.execute_tool("set", json!({"key": "b", "value": "2"}))
        .await
        .expect("set b");

    let result = provider
        .evaluate("KV_COUNT", json!([]))
        .await
        .expect("evaluate KV_COUNT after seeding");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_COUNT result");
    assert_eq!(parsed, json!(2));
}

#[tokio::test(flavor = "current_thread")]
async fn formula_kv_get_returns_value_or_null() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    ext.execute_tool("set", json!({"key": "alpha", "value": "first"}))
        .await
        .expect("set alpha");

    let provider = ext.formula().expect("formula provider should be available");

    let result = provider
        .evaluate("KV_GET", json!(["alpha"]))
        .await
        .expect("evaluate KV_GET alpha");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_GET result");
    assert_eq!(parsed, json!("first"));

    let result = provider
        .evaluate("KV_GET", json!(["missing"]))
        .await
        .expect("evaluate KV_GET missing");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_GET result");
    assert!(parsed.is_null(), "expected JSON null, got {parsed:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn formula_kv_exists_returns_boolean() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    ext.execute_tool("set", json!({"key": "alpha", "value": "v"}))
        .await
        .expect("set alpha");

    let provider = ext.formula().expect("formula provider should be available");

    let result = provider
        .evaluate("KV_EXISTS", json!(["alpha"]))
        .await
        .expect("evaluate KV_EXISTS alpha");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_EXISTS result");
    assert_eq!(parsed, json!(true));

    let result = provider
        .evaluate("KV_EXISTS", json!(["missing"]))
        .await
        .expect("evaluate KV_EXISTS missing");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse KV_EXISTS result");
    assert_eq!(parsed, json!(false));
}

fn kv_keys_schema() -> Vec<column::Def> {
    vec![column::Def {
        name: "key".to_string(),
        alias: "Key".to_string(),
        dtype: "string".to_string(),
    }]
}

#[tokio::test(flavor = "current_thread")]
async fn host_data_cursor_analyze_returns_computed_frequencies() {
    let registry = DataRegistry::new();
    registry.register("kv-keys", kv_keys_schema(), vec![
        json!({"key": "abc"}),
        json!({"key": "ab"}),
        json!({"key": "c"}),
    ]);

    let bytes = std::fs::read(KV_EXTENSION_PATH).expect("read kv extension bytes");
    let ext = Extension::from_bytes_with_data(&bytes, json!({}), registry)
        .await
        .expect("load extension with host data");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["a"]}))
        .await
        .expect("create prefix-tracker block");

    let outcome = provider
        .plan("prefix-tracker", &state, "analyze", json!({}))
        .await
        .expect("plan analyze");
    let freq_json = match outcome {
        plan::Outcome::Computed(freq_json) => freq_json,
        other => panic!("expected Computed, got {other:?}"),
    };

    let freqs: serde_json::Value = serde_json::from_str(&freq_json).expect("parse freq JSON");
    let map = freqs.as_object().expect("freq JSON is object");
    // "abc" + "ab" + "c" → a=2, b=2, c=2.
    assert_eq!(map.get("a").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(map.get("b").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(map.get("c").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(map.len(), 3, "expected exactly 3 distinct chars, got {map:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn host_data_cursor_add_and_analyze_returns_state_update_with_computed() {
    let registry = DataRegistry::new();
    registry.register("kv-keys", kv_keys_schema(), vec![json!({"key": "hello"})]);

    let bytes = std::fs::read(KV_EXTENSION_PATH).expect("read kv extension bytes");
    let ext = Extension::from_bytes_with_data(&bytes, json!({}), registry)
        .await
        .expect("load extension with host data");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["h"]}))
        .await
        .expect("create prefix-tracker block");

    let outcome = provider
        .plan("prefix-tracker", &state, "add_and_analyze", json!({"prefix": "w"}))
        .await
        .expect("plan add_and_analyze");
    let (new_state, freq_json) = match outcome {
        plan::Outcome::StateUpdateWithComputed(new_state, freq_json) => (new_state, freq_json),
        other => panic!("expected StateUpdateWithComputed, got {other:?}"),
    };

    let parsed_state: serde_json::Value = serde_json::from_str(&new_state).expect("parse new state");
    let prefixes = parsed_state
        .get("prefixes")
        .and_then(|v| v.as_array())
        .expect("prefixes array");
    let prefix_strs: Vec<&str> = prefixes.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(prefix_strs, vec!["h", "w"]);

    let freqs: serde_json::Value = serde_json::from_str(&freq_json).expect("parse freq JSON");
    let map = freqs.as_object().expect("freq JSON is object");
    assert_eq!(map.get("h").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(map.get("e").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(map.get("l").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(map.get("o").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test(flavor = "current_thread")]
async fn host_data_cursor_empty_resource_yields_empty_frequencies() {
    let registry = DataRegistry::new();
    registry.register("kv-keys", kv_keys_schema(), Vec::new());

    let bytes = std::fs::read(KV_EXTENSION_PATH).expect("read kv extension bytes");
    let ext = Extension::from_bytes_with_data(&bytes, json!({}), registry)
        .await
        .expect("load extension with host data");

    let provider = ext.block().expect("block provider should be available");
    let state = provider
        .create("prefix-tracker", json!({"prefixes": ["x"]}))
        .await
        .expect("create prefix-tracker block");

    let outcome = provider
        .plan("prefix-tracker", &state, "analyze", json!({}))
        .await
        .expect("plan analyze on empty resource");
    let freq_json = match outcome {
        plan::Outcome::Computed(freq_json) => freq_json,
        other => panic!("expected Computed, got {other:?}"),
    };

    assert_eq!(freq_json, "{}", "empty resource should produce empty frequency map");
}

// -----------------------------------------------------------------------------
// Error-case coverage: every typed call has an error path. These tests assert
// that extension-side failures surface as the correct host-side Error variant.
// -----------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn execute_tool_with_unknown_name_returns_extension_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let err = ext
        .execute_tool("nonexistent_tool", json!({}))
        .await
        .expect_err("unknown tool must fail");
    match err {
        Error::ExtensionError(msg) => {
            assert!(
                msg.contains("Unknown tool: nonexistent_tool"),
                "expected 'Unknown tool: nonexistent_tool' in message, got: {msg}"
            );
        }
        other => panic!("expected ExtensionError, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn execute_tool_with_missing_required_param_returns_extension_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    let err = ext
        .execute_tool("get", json!({}))
        .await
        .expect_err("missing 'key' must fail");
    match err {
        Error::ExtensionError(msg) => {
            assert!(
                msg.contains("Missing 'key' parameter"),
                "expected 'Missing 'key' parameter' in message, got: {msg}"
            );
        }
        other => panic!("expected ExtensionError, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn execute_tool_with_non_object_params_returns_extension_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");

    // Host serializes the raw string as JSON before handing it to the
    // extension; the extension parses it successfully but then rejects the
    // shape when `require_str` fails to find 'key' on a non-object.
    let err = ext
        .execute_tool("get", json!("raw_string"))
        .await
        .expect_err("non-object params must fail");
    assert!(
        matches!(err, Error::ExtensionError(_)),
        "expected ExtensionError, got: {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn block_provider_plan_with_unknown_kind_returns_block_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.block().expect("block provider should be available");

    let err = provider
        .plan("unknown-kind", "{}", "anything", json!({}))
        .await
        .expect_err("unknown kind must fail");
    match err {
        Error::BlockOperation(msg) => {
            assert!(
                msg.contains("Unknown block kind"),
                "expected 'Unknown block kind' in message, got: {msg}"
            );
        }
        other => panic!("expected BlockOperation, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn block_provider_plan_with_unknown_operation_returns_block_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.block().expect("block provider should be available");

    let state = provider
        .create("prefix-tracker", json!({"prefixes": []}))
        .await
        .expect("create prefix-tracker block");

    let err = provider
        .plan("prefix-tracker", &state, "unknown_op", json!({}))
        .await
        .expect_err("unknown operation must fail");
    match err {
        Error::BlockOperation(msg) => {
            assert!(
                msg.contains("Unknown operation"),
                "expected 'Unknown operation' in message, got: {msg}"
            );
        }
        other => panic!("expected BlockOperation, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn block_provider_create_with_unknown_kind_returns_block_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.block().expect("block provider should be available");

    let err = provider
        .create("nonexistent", json!({}))
        .await
        .expect_err("unknown kind must fail");
    assert!(
        matches!(err, Error::BlockOperation(_)),
        "expected BlockOperation, got: {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn block_provider_validate_with_invalid_state_returns_block_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.block().expect("block provider should be available");

    let err = provider
        .validate("prefix-tracker", "not-valid-json")
        .await
        .expect_err("invalid state must fail");
    assert!(
        matches!(err, Error::BlockOperation(_)),
        "expected BlockOperation, got: {err:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn formula_evaluate_with_unknown_name_returns_formula_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.formula().expect("formula provider should be available");

    let err = provider
        .evaluate("NONEXISTENT", json!([]))
        .await
        .expect_err("unknown formula must fail");
    match err {
        Error::FormulaOperation(msg) => {
            assert!(
                msg.contains("Unknown formula"),
                "expected 'Unknown formula' in message, got: {msg}"
            );
        }
        other => panic!("expected FormulaOperation, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn formula_kv_get_with_missing_arg_returns_formula_operation_error() {
    let ext = Extension::load(KV_EXTENSION_PATH, json!({}))
        .await
        .expect("load extension");
    let provider = ext.formula().expect("formula provider should be available");

    let err = provider
        .evaluate("KV_GET", json!([]))
        .await
        .expect_err("missing arg must fail");
    match err {
        Error::FormulaOperation(msg) => {
            assert!(
                msg.contains("expects 1 string argument"),
                "expected 'expects 1 string argument' in message, got: {msg}"
            );
        }
        other => panic!("expected FormulaOperation, got: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn extension_load_with_missing_file_returns_io_error() {
    let err = Extension::load("/nonexistent/path/to.empkg", json!({}))
        .await
        .expect_err("missing file must fail");
    assert!(matches!(err, Error::Io(_)), "expected Io, got: {err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn extension_from_bytes_with_garbage_returns_custom_error() {
    let err = Extension::from_bytes(b"not a valid empkg archive", json!({}))
        .await
        .expect_err("garbage bytes must fail");
    // extract_package wraps every tar/gzip failure in Error::Custom with
    // an "Invalid package" prefix.
    match err {
        Error::Custom(msg) => assert!(
            msg.contains("Invalid package"),
            "expected 'Invalid package' in message, got: {msg}"
        ),
        other => panic!("expected Custom, got: {other:?}"),
    }
}
