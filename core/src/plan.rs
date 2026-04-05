//! Plan execution outcomes (Phase 2 stub; fleshed out in Phase 3).

pub mod data;

use serde::{Deserialize, Serialize};

/// Outcome of a plan step's execution.
///
/// Each variant carries JSON-encoded payloads. The WIT `state-update-with-query`
/// and `state-update-with-computed` variants map to the paired string forms
/// below.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Outcome {
    /// Updated block-type state only.
    StateUpdate(String),
    /// A query to the host (no state change).
    Query(String),
    /// Updated state plus a follow-up query (state, query).
    StateUpdateWithQuery(String, String),
    /// A computed value returned to the host (no state change).
    Computed(String),
    /// Updated state plus a computed value (state, computed).
    StateUpdateWithComputed(String, String),
}
