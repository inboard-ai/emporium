//! WASM extension support.
//!
//! Phase 1 stub: the old string-based WIT bindings have been removed. Phase 2
//! rewrites this file end-to-end against the new typed provider-based WIT.
//! Public API surface (`Extension::new`, `Extension::into_sipper`) is preserved
//! as a stub so `extension.rs` continues to compile during Phase 1.

use futures::channel::mpsc;
use sipper::{Sipper, sipper};

use crate::data::{Command, Event, Id};

/// Public type aliases for easier consumer access
pub type Sender = futures::channel::mpsc::UnboundedSender<Command>;
pub type Receiver = futures::channel::mpsc::UnboundedReceiver<Command>;

/// Internal WASM extension wrapper.
/// The public API is in [`crate::extension::Extension`].
#[derive(Clone)]
pub(crate) struct Extension {
    id: Id,
    wasm_bytes: Vec<u8>,
    config: String,
}

impl Extension {
    /// Create a new WASM extension.
    pub(crate) fn new(id: Id, wasm_bytes: Vec<u8>, config: String) -> Self {
        Self { id, wasm_bytes, config }
    }

    /// Convert the extension into a sipper that emits responses.
    ///
    /// Phase 1 stub: immediately emits a `Connected` event with a discarded
    /// sender and exits. Phase 2 replaces this with a typed worker loop.
    pub(crate) fn into_sipper(self) -> impl Sipper<(), Event> {
        let _ = self.wasm_bytes;
        let _ = self.config;
        let id = self.id;
        sipper(move |mut output| async move {
            let (tx, _rx): (Sender, Receiver) = mpsc::unbounded();
            output.send(Event::Connected(tx)).await;
            eprintln!("Extension {} stub sipper exiting (Phase 1)", id);
        })
    }
}

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmExtension").field("id", &self.id).finish()
    }
}
