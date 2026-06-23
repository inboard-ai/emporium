//! Host-side data-provider proxy.
//!
//! [`Provider`] is a thin, cheap-to-clone newtype over the worker channel
//! shared with the parent [`Extension`](crate::Extension). A fresh provider is
//! minted by [`Extension::data`](crate::Extension::data) whenever the extension
//! was loaded against the `data-extension` world (which exports
//! `data-provider`).
//!
//! This is the declarative, UI-builder-facing sibling to the imperative tool
//! API — it drives the dashboard/tile builder (pick a source -> typed param form
//! or drill-down -> known output schema -> fetch) rather than the LLM. See
//! `docs/ui-data-capability-protocol.md`.

use emporium_core::data;
use tokio::sync::{mpsc, oneshot};

use crate::Error;
use crate::wasm::Request;

/// Typed client for an extension's `data-provider` interface.
///
/// Structure (catalog, output schema, selection) crosses as typed values; bulk
/// result data rides inside [`data::FetchResult::arrow_ipc`] as an Arrow IPC
/// buffer (the same wire as the tool data-frame output). Failures the extension
/// reports surface as [`Error::Data`] carrying the typed [`data::Error`]
/// taxonomy, so the host can render per case (reconnect, rate-limit, retry); a
/// dead worker surfaces [`Error::ExtensionCrashed`].
#[derive(Clone)]
pub struct Provider {
    sender: mpsc::UnboundedSender<Request>,
}

impl Provider {
    /// Construct a provider wrapping a cloned request sender. Crate-private:
    /// callers obtain providers via [`Extension::data`](crate::Extension::data).
    pub(crate) fn new(sender: mpsc::UnboundedSender<Request>) -> Self {
        Provider { sender }
    }

    /// List the data sources this configured instance exposes. For a static
    /// catalog the host may instead read the manifest `[[data_sources]]` mirror;
    /// [`is_catalog_dynamic`](Self::is_catalog_dynamic) says which is
    /// authoritative.
    pub async fn list_sources(&self) -> Result<Vec<data::Source>, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::ListSources { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Whether the source catalog depends on the configured credentials (so the
    /// host must call [`list_sources`](Self::list_sources)) rather than being a
    /// static manifest mirror.
    pub async fn is_catalog_dynamic(&self) -> Result<bool, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::IsCatalogDynamic { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Browse one level of a source's drill-down tree. `path` is the node ids
    /// from root to the current level (empty = root); `cursor` continues a prior
    /// page. v1b.
    pub async fn browse(
        &self,
        source: &str,
        path: &[String],
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<data::Page, Error> {
        let path = serde_json::to_string(path)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::Browse {
                source: source.to_string(),
                path,
                cursor: cursor.map(str::to_string),
                limit,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Resolve the output schema for a settled selection — used when a source's
    /// output contract is [`Resolved`](data::source::Output::Resolved). v1b.
    pub async fn describe(&self, source: &str, selection: &data::Selection) -> Result<data::source::Shape, Error> {
        let selection = selection.to_json()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::Describe {
                source: source.to_string(),
                selection,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Fetch data for a settled selection. The extension owns query
    /// construction; the host stays warehouse-agnostic. The result's rows ride
    /// as an Arrow IPC buffer in [`data::FetchResult::arrow_ipc`].
    pub async fn fetch(&self, source: &str, selection: &data::Selection) -> Result<data::FetchResult, Error> {
        let selection = selection.to_json()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::Fetch {
                source: source.to_string(),
                selection,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }
}

// Compile-time guarantee: `Provider` stays `Send + Sync + Clone` so host
// callers can share it freely across tasks.
const _: fn() = || {
    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
    assert_send_sync_clone::<Provider>();
};

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn data_provider_send_error_maps_to_extension_crashed() {
        // Drop the receiver before issuing requests: every send fails, so every
        // method must surface ExtensionCrashed without a live worker.
        let (tx, rx) = mpsc::unbounded_channel::<Request>();
        drop(rx);
        let provider = Provider::new(tx);
        let sel = data::Selection::default();

        assert!(matches!(
            provider.list_sources().await.expect_err("list_sources"),
            Error::ExtensionCrashed
        ));
        assert!(matches!(
            provider.is_catalog_dynamic().await.expect_err("is_catalog_dynamic"),
            Error::ExtensionCrashed
        ));
        assert!(matches!(
            provider.browse("src", &[], None, 50).await.expect_err("browse"),
            Error::ExtensionCrashed
        ));
        assert!(matches!(
            provider.describe("src", &sel).await.expect_err("describe"),
            Error::ExtensionCrashed
        ));
        assert!(matches!(
            provider.fetch("src", &sel).await.expect_err("fetch"),
            Error::ExtensionCrashed
        ));
    }
}
