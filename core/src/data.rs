//! Canonical data-capability types — the host-side mirror of the
//! `data-provider` WIT interface.
//!
//! These are the declarative, UI-builder-facing surface (a sibling to the
//! imperative [`tool`](crate::tool) types), introduced by protocol v1a. See
//! `docs/ui-data-capability-protocol.md`.
//!
//! Bulk result data does NOT live here — it crosses as an Arrow IPC buffer (see
//! [`FetchResult::arrow_ipc`], decoded via [`tool::data_frame`](crate::tool)).
//! These types carry only *structure*: the catalog, the input shape, the output
//! schema, the selection, and the typed error taxonomy. Declarations stay typed;
//! the Arrow buffer carries the cells — the two never mix.

use serde::{Deserialize, Serialize};

use crate::column;

/// One pullable source within a configured extension instance.
///
/// `input` says how the host gathers what [`fetch`](crate) needs; `output` says
/// what the host knows about the result shape *before* fetch — the keystone that
/// lets the measure/axis pickers light up immediately for a [`Known`] schema.
///
/// [`Known`]: OutputContract::Known
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// Stable identifier within the extension.
    pub id: String,
    /// Human-facing name shown in the picker.
    pub display_name: String,
    /// One-line description of what the source returns.
    pub description: String,
    /// How the host gathers fetch inputs.
    pub input: InputSpec,
    /// What the host knows about output shape before fetch.
    pub output: OutputContract,
}

/// How the host gathers what `fetch` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputSpec {
    /// Typed param form driven by a JSON Schema (parsed). v1a.
    Params(serde_json::Value),
    /// Drill-down tree via `browse`. v1b.
    Browse,
    /// A browse selection plus params layered on top. v1b+.
    ParamsAndBrowse(serde_json::Value),
    /// Free-form text the extension compiles into a query. v1b.
    FreeQuery(FreeQuerySpec),
}

/// Editor hint for a [`InputSpec::FreeQuery`] source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeQuerySpec {
    /// Language hint for the host editor (`"sql"`, `"promql"`, `"text"`, …).
    pub language: String,
    /// Placeholder text for the empty editor.
    pub placeholder: String,
}

/// What the host knows about output shape before fetch — the keystone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputContract {
    /// Statically known regardless of input; the host seeds schema immediately.
    Known(OutputSpec),
    /// Knowable only after the selection is made; the host calls
    /// `output_schema(source, selection)` once the user has chosen. v1b.
    Resolved,
}

/// The shape of a source's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputSpec {
    /// Tabular relation with a known column set.
    Rows(Vec<column::Def>),
    /// A single headline value.
    Scalar(column::Def),
    /// A complete resource the host references without reshaping. v2.
    Finished,
}

/// One node in a `browse` drill-down. v1b.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Opaque id appended to the browse path.
    pub id: String,
    /// Display label.
    pub label: String,
    /// `true` => descendable (call `browse` again with this appended to `path`);
    /// `false` => a selectable leaf.
    pub branch: bool,
    /// Present on leaves whose output shape is known at browse time, letting the
    /// host skip a separate `output_schema` round-trip.
    pub output: Option<OutputSpec>,
}

/// One page of `browse` results. v1b.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// The nodes in this page.
    pub nodes: Vec<Node>,
    /// Opaque continuation; `None` => no more pages.
    pub next_cursor: Option<String>,
}

/// Everything `fetch` / `output_schema` need, host -> extension.
///
/// Crosses the WIT boundary as a JSON string; [`Selection::to_json`] /
/// [`Selection::from_json`] are the canonical (de)serializers. For `params`
/// sources `path` is empty and `params` is populated; for pure `browse` the
/// reverse; for `free-query`, `query` is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Selection {
    /// Browse node ids, root -> leaf. Empty for `params` / `free-query`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    /// Param-input values. Absent for pure browse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Free-query text, when the source is `free-query`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Pagination continuation from a prior fetch. v1b.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl Selection {
    /// Serialize to the JSON string carried across the WIT boundary.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse from the JSON string carried across the WIT boundary.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Result of a `fetch`.
///
/// Bulk data is the Arrow IPC buffer (the same wire as
/// [`tool::DataFrame`](crate::tool::DataFrame) — decode with the
/// `tool::data_frame` codec). For a `Finished` output `arrow_ipc` is empty and
/// `resource_id` carries the landed reference.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// Columnar result as an Arrow IPC byte buffer; empty for `Finished`.
    pub arrow_ipc: Vec<u8>,
    /// Set only for `Finished` outputs.
    pub resource_id: Option<String>,
    /// Pagination continuation; `None` => complete. Always `None` in v1a.
    pub next_cursor: Option<String>,
}

/// Rate-limit detail carried by [`Error::RateLimited`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Human-readable message.
    pub message: String,
    /// Seconds to wait before retrying, if the extension knows.
    pub retry_after_secs: Option<u32>,
}

/// Typed error taxonomy reported by a data-provider — mirrors the WIT
/// `data-error` variant so the host can render per case (e.g. [`AuthFailed`] ->
/// "Reconnect", [`RateLimited`] -> "try again in N s") instead of a generic
/// "couldn't load".
///
/// [`AuthFailed`]: Error::AuthFailed
/// [`RateLimited`]: Error::RateLimited
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum Error {
    /// Bad or expired credentials — the host should prompt to reconnect.
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// A source, path, or table that no longer exists.
    #[error("not found: {0}")]
    NotFound(String),
    /// The host built a [`Selection`] the extension rejects.
    #[error("invalid selection: {0}")]
    InvalidSelection(String),
    /// Back off and retry later.
    #[error("rate limited: {}", .0.message)]
    RateLimited(RateLimit),
    /// A transient failure (network blip); retry may succeed.
    #[error("transient error: {0}")]
    Transient(String),
    /// The capability is not implemented for this source.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Catch-all; the host shows a generic message and logs the detail.
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_json_roundtrips_and_omits_empty_fields() {
        let sel = Selection {
            path: vec!["conn_prod".into(), "orders".into()],
            params: Some(serde_json::json!({ "row_limit": 1000 })),
            query: None,
            cursor: None,
        };
        let json = sel.to_json().unwrap();
        // empty/None fields are skipped on the wire.
        assert!(!json.contains("query"));
        assert!(!json.contains("cursor"));
        let back = Selection::from_json(&json).unwrap();
        assert_eq!(back.path, sel.path);
        assert_eq!(back.params, sel.params);
    }

    #[test]
    fn empty_selection_serializes_to_empty_object() {
        assert_eq!(Selection::default().to_json().unwrap(), "{}");
    }

    #[test]
    fn output_spec_rows_roundtrips() {
        let spec = OutputSpec::Rows(vec![column::Def {
            name: "close".into(),
            alias: "Close".into(),
            dtype: "number".into(),
        }]);
        let json = serde_json::to_string(&spec).unwrap();
        let back: OutputSpec = serde_json::from_str(&json).unwrap();
        match back {
            OutputSpec::Rows(cols) => assert_eq!(cols[0].name, "close"),
            other => panic!("expected Rows, got {other:?}"),
        }
    }
}
