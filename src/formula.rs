//! Host-side formula-provider proxy.
//!
//! [`Provider`] is a thin, cheap-to-clone newtype over the worker channel
//! shared with the parent [`Extension`](crate::Extension). A fresh provider
//! is minted by [`Extension::formula`](crate::Extension::formula) whenever
//! the extension was loaded against a world that exports `formula-provider`.

use emporium_core::formula;
use tokio::sync::{mpsc, oneshot};

use crate::Error;
use crate::wasm::Request;

/// Typed client for an extension's `formula-provider` interface.
///
/// Methods serialize JSON payloads before crossing the WASM boundary and
/// return the canonical [`emporium_core::formula`] types (or the raw
/// JSON-encoded result string) on return. Errors returned by the extension
/// are surfaced as [`Error::FormulaOperation`]. If the worker thread has
/// exited, calls observe [`Error::ExtensionCrashed`].
#[derive(Clone)]
pub struct Provider {
    sender: mpsc::UnboundedSender<Request>,
}

impl Provider {
    /// Construct a provider wrapping a cloned request sender. Crate-private:
    /// callers obtain providers via [`Extension::formula`](crate::Extension::formula).
    pub(crate) fn new(sender: mpsc::UnboundedSender<Request>) -> Self {
        Provider { sender }
    }

    /// Enumerate formulas this extension exports.
    pub async fn defs(&self) -> Result<Vec<formula::Def>, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::FormulaDefs { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Evaluate a named formula with JSON arguments. The `args` value is
    /// typically a JSON array of positional arguments. Returns the raw
    /// JSON-encoded result string the extension produced — callers parse
    /// it as whatever shape the formula promises.
    pub async fn evaluate(&self, name: &str, args: serde_json::Value) -> Result<String, Error> {
        let args_str = serde_json::to_string(&args)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(Request::FormulaEvaluate {
                name: name.to_string(),
                args: args_str,
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
    fn formula_provider_newtype_is_clone_and_cheap() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Request>();
        let provider = Provider::new(tx);
        let clone = provider.clone();

        // Send a FormulaDefs request via the clone's shared sender. We
        // bypass the async method (no runtime in scope) and hit the
        // sender directly — both clones must route to the same receiver.
        let (reply_tx, _reply_rx) = oneshot::channel();
        clone
            .sender
            .send(Request::FormulaDefs { reply: reply_tx })
            .expect("clone's sender should be live");

        let observed = rx.try_recv().expect("receiver should see the clone's frame");
        assert!(
            matches!(observed, Request::FormulaDefs { .. }),
            "expected FormulaDefs variant"
        );

        // And the original still routes to the same receiver after the
        // clone is dropped.
        drop(clone);
        let (reply_tx, _reply_rx) = oneshot::channel();
        provider
            .sender
            .send(Request::FormulaDefs { reply: reply_tx })
            .expect("original sender should still be live");
        let observed = rx.try_recv().expect("receiver should see the original's frame");
        assert!(matches!(observed, Request::FormulaDefs { .. }));

        // Dropping the last clone closes the channel.
        drop(provider);
        assert!(rx.is_closed(), "channel should close once all clones drop");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn formula_provider_send_error_maps_to_extension_crashed() {
        // Drop the receiver before issuing requests: every send fails, every
        // call must surface ExtensionCrashed. This exercises the error
        // propagation path without a live worker.
        let (tx, rx) = mpsc::unbounded_channel::<Request>();
        drop(rx);
        let provider = Provider::new(tx);

        let err = provider.defs().await.expect_err("defs should fail");
        assert!(
            matches!(err, Error::ExtensionCrashed),
            "defs: expected ExtensionCrashed, got: {err:?}"
        );

        let err = provider
            .evaluate("KV_GET", serde_json::json!([]))
            .await
            .expect_err("evaluate should fail");
        assert!(
            matches!(err, Error::ExtensionCrashed),
            "evaluate: expected ExtensionCrashed, got: {err:?}"
        );
    }
}
