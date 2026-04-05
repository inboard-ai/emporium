//! Host-side implementation of the `host-data` WIT import.
//!
//! Extensions loaded against the `full-extension` world can stream-read
//! host-managed data by opening a [`Cursor`] resource and requesting batches
//! from it. The host-side data is held in a [`DataRegistry`] — a cheap,
//! clone-friendly handle backed by an `Arc<RwLock<...>>`. Phase 5 keeps the
//! query parameter opaque (no filtering); real predicate pushdown is Phase 6+.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use emporium_core::column;

/// A registry of host-managed data resources that extensions can stream via
/// cursors. Populated by the host application before loading an extension.
///
/// Cloning a `DataRegistry` is cheap: clones share the same inner storage
/// via `Arc<RwLock<...>>`, so registering a resource on one clone is visible
/// to every other clone (and to any worker thread holding a clone).
#[derive(Clone, Default)]
pub struct DataRegistry {
    inner: Arc<RwLock<HashMap<String, ResourceData>>>,
}

/// Row-oriented data plus its schema, held under a stable resource id.
#[derive(Clone)]
struct ResourceData {
    schema: Vec<column::Def>,
    rows: Vec<serde_json::Value>,
}

impl DataRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a resource with its schema and row data. Subsequent calls
    /// with the same `id` replace the prior entry.
    pub fn register(&self, id: impl Into<String>, schema: Vec<column::Def>, rows: Vec<serde_json::Value>) {
        let id = id.into();
        let data = ResourceData { schema, rows };
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(id, data);
    }

    /// Return a clone of the schema registered under `id`, if any.
    pub(crate) fn schema(&self, id: &str) -> Option<Vec<column::Def>> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(id).map(|rd| rd.schema.clone())
    }

    /// Return the total number of rows registered under `id`, if any.
    pub(crate) fn row_count(&self, id: &str) -> Option<u64> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(id).map(|rd| rd.rows.len() as u64)
    }

    /// Return up to `batch_size` rows starting at `offset`, or `None` if
    /// the resource id is unknown. An empty `Vec` signals EOF to callers.
    pub(crate) fn slice(&self, id: &str, offset: usize, batch_size: usize) -> Option<Vec<serde_json::Value>> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let rd = guard.get(id)?;
        let len = rd.rows.len();
        if offset >= len {
            return Some(Vec::new());
        }
        let end = (offset + batch_size).min(len);
        Some(rd.rows[offset..end].to_vec())
    }
}

impl std::fmt::Debug for DataRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = match self.inner.read() {
            Ok(g) => g.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        };
        f.debug_struct("DataRegistry").field("resources", &count).finish()
    }
}

/// Host-side state for a single open cursor. Lifetimes are managed by
/// wasmtime's `ResourceTable` — pushed on `open`, deleted on `close`/`drop`.
///
/// Public to the crate's users because wasmtime's `bindgen!` `with:` config
/// requires the remapped resource type to be reachable from the bindings
/// module, but the fields are crate-internal.
#[derive(Debug)]
pub struct Cursor {
    /// Resource id the cursor is streaming from.
    pub(crate) resource_id: String,
    /// Next row index to emit on `next-batch`.
    pub(crate) offset: usize,
    /// Opaque query string retained for future filtering (Phase 6+). Not
    /// interpreted in Phase 5.
    #[allow(dead_code)]
    pub(crate) query: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_schema() -> Vec<column::Def> {
        vec![column::Def {
            name: "key".into(),
            alias: "Key".into(),
            dtype: "string".into(),
        }]
    }

    fn sample_rows() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({"key": "a"}),
            serde_json::json!({"key": "b"}),
            serde_json::json!({"key": "c"}),
            serde_json::json!({"key": "d"}),
        ]
    }

    #[test]
    fn register_and_slice_returns_rows_and_advances() {
        let reg = DataRegistry::new();
        reg.register("keys", sample_schema(), sample_rows());

        assert_eq!(reg.row_count("keys"), Some(4));
        assert_eq!(reg.schema("keys").map(|s| s.len()), Some(1));

        let first = reg.slice("keys", 0, 2).expect("resource exists");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], serde_json::json!({"key": "a"}));
        assert_eq!(first[1], serde_json::json!({"key": "b"}));

        let second = reg.slice("keys", 2, 2).expect("resource exists");
        assert_eq!(second.len(), 2);
        assert_eq!(second[0], serde_json::json!({"key": "c"}));
        assert_eq!(second[1], serde_json::json!({"key": "d"}));

        // Past-end slice yields an empty vec (EOF), not None.
        let eof = reg.slice("keys", 4, 2).expect("resource exists");
        assert!(eof.is_empty());
    }

    #[test]
    fn unknown_resource_returns_none() {
        let reg = DataRegistry::new();
        assert!(reg.slice("missing", 0, 10).is_none());
        assert!(reg.schema("missing").is_none());
        assert!(reg.row_count("missing").is_none());
    }

    #[test]
    fn slice_larger_than_remaining_truncates() {
        let reg = DataRegistry::new();
        reg.register("keys", sample_schema(), sample_rows());
        let rows = reg.slice("keys", 3, 100).expect("resource exists");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], serde_json::json!({"key": "d"}));
    }

    #[test]
    fn register_replaces_existing_entry() {
        let reg = DataRegistry::new();
        reg.register("x", sample_schema(), sample_rows());
        assert_eq!(reg.row_count("x"), Some(4));
        reg.register("x", sample_schema(), vec![serde_json::json!({"key": "z"})]);
        assert_eq!(reg.row_count("x"), Some(1));
    }

    #[test]
    fn data_registry_clones_share_backing_store() {
        let reg = DataRegistry::new();
        let clone = reg.clone();
        reg.register("shared", sample_schema(), sample_rows());
        assert_eq!(clone.row_count("shared"), Some(4));
    }
}
