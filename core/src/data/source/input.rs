//! Input shape for a data source — how the host gathers what `fetch` needs.

use serde::{Deserialize, Serialize};

/// How the host gathers what `fetch` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Input {
    /// Typed param form driven by a JSON Schema (parsed). v1a.
    Params(serde_json::Value),
    /// Drill-down tree via `browse`. v1b.
    Browse,
    /// A browse selection plus params layered on top. v1b+.
    ParamsAndBrowse(serde_json::Value),
    /// Free-form text the extension compiles into a query. v1b.
    FreeQuery(FreeQuery),
}

/// Editor hint for a [`Input::FreeQuery`] source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeQuery {
    /// Language hint for the host editor (`"sql"`, `"promql"`, `"text"`, ...).
    pub language: String,
    /// Placeholder text for the empty editor.
    pub placeholder: String,
}
