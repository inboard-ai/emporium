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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_state_update_carries_payload() {
        let outcome = Outcome::StateUpdate("state-json".to_string());
        match outcome {
            Outcome::StateUpdate(s) => assert_eq!(s, "state-json"),
            _ => panic!("expected StateUpdate"),
        }
    }

    #[test]
    fn outcome_query_carries_payload() {
        let outcome = Outcome::Query("query-json".to_string());
        match outcome {
            Outcome::Query(q) => assert_eq!(q, "query-json"),
            _ => panic!("expected Query"),
        }
    }

    #[test]
    fn outcome_state_update_with_query_carries_pair() {
        let outcome = Outcome::StateUpdateWithQuery("state".to_string(), "query".to_string());
        match outcome {
            Outcome::StateUpdateWithQuery(s, q) => {
                assert_eq!(s, "state");
                assert_eq!(q, "query");
            }
            _ => panic!("expected StateUpdateWithQuery"),
        }
    }

    #[test]
    fn outcome_computed_carries_payload() {
        let outcome = Outcome::Computed("computed".to_string());
        match outcome {
            Outcome::Computed(c) => assert_eq!(c, "computed"),
            _ => panic!("expected Computed"),
        }
    }

    #[test]
    fn outcome_state_update_with_computed_carries_pair() {
        let outcome = Outcome::StateUpdateWithComputed("state".to_string(), "computed".to_string());
        match outcome {
            Outcome::StateUpdateWithComputed(s, c) => {
                assert_eq!(s, "state");
                assert_eq!(c, "computed");
            }
            _ => panic!("expected StateUpdateWithComputed"),
        }
    }

    #[test]
    fn outcome_serde_roundtrip_preserves_variant() {
        let cases = [
            Outcome::StateUpdate("s".to_string()),
            Outcome::Query("q".to_string()),
            Outcome::StateUpdateWithQuery("s".to_string(), "q".to_string()),
            Outcome::Computed("c".to_string()),
            Outcome::StateUpdateWithComputed("s".to_string(), "c".to_string()),
        ];
        for original in &cases {
            let encoded = serde_json::to_string(original).unwrap();
            let decoded: Outcome = serde_json::from_str(&encoded).unwrap();
            match (original, &decoded) {
                (Outcome::StateUpdate(a), Outcome::StateUpdate(b)) => assert_eq!(a, b),
                (Outcome::Query(a), Outcome::Query(b)) => assert_eq!(a, b),
                (Outcome::StateUpdateWithQuery(a1, a2), Outcome::StateUpdateWithQuery(b1, b2)) => {
                    assert_eq!(a1, b1);
                    assert_eq!(a2, b2);
                }
                (Outcome::Computed(a), Outcome::Computed(b)) => assert_eq!(a, b),
                (Outcome::StateUpdateWithComputed(a1, a2), Outcome::StateUpdateWithComputed(b1, b2)) => {
                    assert_eq!(a1, b1);
                    assert_eq!(a2, b2);
                }
                _ => panic!("variant mismatch after roundtrip"),
            }
        }
    }

    #[test]
    fn outcome_match_requires_named_catch_all() {
        // `Outcome` is `#[non_exhaustive]`; within this crate all variants are
        // known, but this documents the downstream-crate pattern of using a
        // named catch-all arm. Clippy flags the arm as unreachable within the
        // same crate — allow it to keep the documentation value.
        let outcome = Outcome::Query("q".to_string());
        #[allow(unreachable_patterns)]
        let label = match &outcome {
            Outcome::StateUpdate(_) => "state-update",
            Outcome::Query(_) => "query",
            Outcome::StateUpdateWithQuery(_, _) => "state-update-with-query",
            Outcome::Computed(_) => "computed",
            Outcome::StateUpdateWithComputed(_, _) => "state-update-with-computed",
            other => {
                let _ = other;
                "unknown"
            }
        };
        assert_eq!(label, "query");
    }
}
