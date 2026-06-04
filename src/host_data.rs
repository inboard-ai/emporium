//! Host-side implementation of the `host-data` WIT import.
//!
//! Extensions loaded against the `full-extension` world can stream-read
//! host-managed data by opening a [`Cursor`] resource and requesting batches
//! from it. The host-side data is held in a [`DataRegistry`] — a cheap,
//! clone-friendly handle backed by an `Arc<RwLock<...>>`. Phase 5 keeps the
//! query parameter opaque (no filtering); real predicate pushdown is Phase 6+.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use emporium_core::tool::data_frame::StoredFrame;
use emporium_core::{Error, column};

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

/// A decoded Arrow frame plus its declared schema, held under a stable resource
/// id. The frame is decoded once at [`DataRegistry::register`]; windows are
/// sliced zero-copy per cursor `next-batch`.
#[derive(Clone)]
struct ResourceData {
    schema: Vec<column::Def>,
    frame: StoredFrame,
}

impl DataRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a resource from its declared schema and an Arrow IPC buffer.
    /// The buffer is decoded once into a [`StoredFrame`]; subsequent calls with
    /// the same `id` replace the prior entry. Errors if the buffer is not a
    /// valid Arrow IPC stream.
    pub fn register(&self, id: impl Into<String>, schema: Vec<column::Def>, frame_ipc: &[u8]) -> Result<(), Error> {
        let id = id.into();
        let frame = StoredFrame::from_ipc(frame_ipc)?;
        let data = ResourceData { schema, frame };
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(id, data);
        Ok(())
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
    pub fn row_count(&self, id: &str) -> Option<u64> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(id).map(|rd| rd.frame.row_count() as u64)
    }

    /// Return a cheap handle to the decoded frame for `id` (polars frames are
    /// `Arc`-backed, so the clone bumps refcounts — it does not copy buffers),
    /// or `None` if the resource id is unknown. The cursor slices windows off
    /// the returned handle via [`StoredFrame::batch_ipc`].
    pub(crate) fn frame(&self, id: &str) -> Option<StoredFrame> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(id).map(|rd| rd.frame.clone())
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

    /// Four single-column rows, encoded to an Arrow IPC buffer the way an
    /// extension would hand them in.
    fn sample_ipc() -> Vec<u8> {
        let data = serde_json::json!([
            {"key": "a"}, {"key": "b"}, {"key": "c"}, {"key": "d"},
        ]);
        emporium_core::tool::data_frame::to_arrow_ipc(&sample_schema(), &data).expect("encode sample frame")
    }

    /// Decode a batch buffer back to a polars frame to assert its height.
    fn batch_height(bytes: &[u8]) -> usize {
        emporium_core::tool::data_frame::from_arrow_ipc(bytes)
            .expect("decode batch")
            .height()
    }

    #[test]
    fn register_and_batch_windows_advance() {
        let reg = DataRegistry::new();
        reg.register("keys", sample_schema(), &sample_ipc()).expect("register");

        assert_eq!(reg.row_count("keys"), Some(4));
        assert_eq!(reg.schema("keys").map(|s| s.len()), Some(1));

        let frame = reg.frame("keys").expect("resource exists");

        let (b0, n0) = frame.batch_ipc(0, 2).unwrap().expect("first window");
        assert_eq!(n0, 2);
        assert_eq!(batch_height(&b0), 2);

        let (b1, n1) = frame.batch_ipc(2, 2).unwrap().expect("second window");
        assert_eq!(n1, 2);
        assert_eq!(batch_height(&b1), 2);

        // Past-end window yields None (EOF).
        assert!(frame.batch_ipc(4, 2).unwrap().is_none());
    }

    #[test]
    fn unknown_resource_returns_none() {
        let reg = DataRegistry::new();
        assert!(reg.frame("missing").is_none());
        assert!(reg.schema("missing").is_none());
        assert!(reg.row_count("missing").is_none());
    }

    #[test]
    fn tail_window_truncates_to_remaining() {
        let reg = DataRegistry::new();
        reg.register("keys", sample_schema(), &sample_ipc()).expect("register");
        let frame = reg.frame("keys").expect("resource exists");
        let (bytes, n) = frame.batch_ipc(3, 100).unwrap().expect("tail window");
        assert_eq!(n, 1);
        assert_eq!(batch_height(&bytes), 1);
    }

    #[test]
    fn register_replaces_existing_entry() {
        let reg = DataRegistry::new();
        reg.register("x", sample_schema(), &sample_ipc()).expect("register");
        assert_eq!(reg.row_count("x"), Some(4));
        let one = emporium_core::tool::data_frame::to_arrow_ipc(&sample_schema(), &serde_json::json!([{"key": "z"}]))
            .expect("encode one row");
        reg.register("x", sample_schema(), &one).expect("re-register");
        assert_eq!(reg.row_count("x"), Some(1));
    }

    #[test]
    fn empty_buffer_registers_as_zero_rows() {
        let reg = DataRegistry::new();
        reg.register("empty", sample_schema(), &[])
            .expect("register placeholder");
        assert_eq!(reg.row_count("empty"), Some(0));
        assert!(reg.frame("empty").unwrap().batch_ipc(0, 10).unwrap().is_none());
    }

    #[test]
    fn register_rejects_garbage_ipc() {
        let reg = DataRegistry::new();
        assert!(reg.register("bad", sample_schema(), b"not an arrow stream").is_err());
    }

    #[test]
    fn data_registry_clones_share_backing_store() {
        let reg = DataRegistry::new();
        let clone = reg.clone();
        reg.register("shared", sample_schema(), &sample_ipc())
            .expect("register");
        assert_eq!(clone.row_count("shared"), Some(4));
    }
}
