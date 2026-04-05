//! Host-side block-provider proxy.
//!
//! [`Provider`] is a thin, cheap-to-clone newtype over the worker channel
//! shared with the parent [`Extension`](crate::Extension). A fresh provider
//! is minted by [`Extension::block`](crate::Extension::block) whenever the
//! extension was loaded against a world that exports `block-provider`.

use emporium_core::{block, plan};
use tokio::sync::{mpsc, oneshot};

use crate::Error;
use crate::wasm::Request;

/// Typed client for an extension's `block-provider` interface.
///
/// Methods serialize JSON payloads before crossing the WASM boundary and
/// deserialize canonical [`emporium_core`] types on return. Errors returned
/// by the extension are surfaced as [`Error::BlockOperation`]. If the worker
/// thread has exited, calls observe [`Error::ExtensionCrashed`].
#[derive(Clone)]
pub struct Provider {
    sender: mpsc::UnboundedSender<Request>,
}

impl Provider {
    /// Construct a provider wrapping a cloned request sender. Crate-private:
    /// callers obtain providers via [`Extension::block`](crate::Extension::block).
    pub(crate) fn new(sender: mpsc::UnboundedSender<Request>) -> Self {
        Provider { sender }
    }

    /// Enumerate block types this extension exports.
    pub async fn types(&self) -> Result<Vec<block::TypeInfo>, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::BlockTypes { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Create a new block of `kind` from JSON input. Returns the encoded
    /// state JSON string the extension produced.
    pub async fn create(&self, kind: &str, input: serde_json::Value) -> Result<String, Error> {
        let input_str = serde_json::to_string(&input)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::BlockCreate {
                kind: kind.to_string(),
                input: input_str,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Run `operation` against an existing block state. Returns the plan
    /// outcome the extension emitted.
    pub async fn plan(
        &self,
        kind: &str,
        state: &str,
        operation: &str,
        input: serde_json::Value,
    ) -> Result<plan::Outcome, Error> {
        let input_str = serde_json::to_string(&input)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::BlockPlan {
                kind: kind.to_string(),
                state: state.to_string(),
                operation: operation.to_string(),
                input: input_str,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Validate an encoded block state under its `kind`'s schema.
    pub async fn validate(&self, kind: &str, state: &str) -> Result<(), Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::BlockValidate {
                kind: kind.to_string(),
                state: state.to_string(),
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

    #[test]
    fn block_provider_newtype_is_clone_and_cheap() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Request>();
        let provider = Provider::new(tx);
        let clone = provider.clone();

        // Send a BlockTypes request via the clone's shared sender. We
        // bypass the async method (no runtime in scope) and hit the
        // sender directly — both clones must route to the same receiver.
        let (reply_tx, _reply_rx) = oneshot::channel();
        clone
            .sender
            .send(Request::BlockTypes { reply: reply_tx })
            .expect("clone's sender should be live");

        let observed = rx.try_recv().expect("receiver should see the clone's frame");
        assert!(
            matches!(observed, Request::BlockTypes { .. }),
            "expected BlockTypes variant"
        );

        // And the original still routes to the same receiver after the
        // clone is dropped.
        drop(clone);
        let (reply_tx, _reply_rx) = oneshot::channel();
        provider
            .sender
            .send(Request::BlockTypes { reply: reply_tx })
            .expect("original sender should still be live");
        let observed = rx.try_recv().expect("receiver should see the original's frame");
        assert!(matches!(observed, Request::BlockTypes { .. }));

        // Dropping the last clone closes the channel.
        drop(provider);
        assert!(rx.is_closed(), "channel should close once all clones drop");
    }
}
