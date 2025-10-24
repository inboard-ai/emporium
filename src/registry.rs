//! Manage extensions and send them messages.
use crate::{Error, Extension, Id, Message, Response};
use futures::channel::mpsc;
use futures::{Stream, StreamExt};
use std::collections::HashMap;

pub struct Registry {
    /// Loaded extensions
    extensions: HashMap<Id, mpsc::UnboundedSender<Message>>,
    /// Event sender that extensions use
    event_tx: mpsc::UnboundedSender<(Id, Response)>,
    /// Event receiver that extensions use
    event_rx: mpsc::UnboundedReceiver<(Id, Response)>,
}

impl Registry {
    /// Create a new registry
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded();
        Self {
            extensions: HashMap::new(),
            event_tx,
            event_rx,
        }
    }

    /// Register an extension with the registry
    pub async fn register(&mut self, id: Id, extension: Extension) -> Result<(), Error> {
        if self.extensions.contains_key(&id) {
            return Err(Error::RegistryAlreadyExists(format!(
                "Extension {} already registered",
                id
            )));
        }

        // Create the sipper
        let (sipper, msg_tx) = extension.into_sipper();

        // Forward events from this extension to the registry's event stream
        let ext_id = id.clone();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            futures::pin_mut!(sipper);
            while let Some(response) = sipper.next().await {
                let _ = event_tx.unbounded_send((ext_id.clone(), response));
            }
            sipper.await;
        });

        self.extensions.insert(id, msg_tx);
        Ok(())
    }

    /// Send a message to a specific extension
    pub fn send_message(&self, extension_id: &Id, message: Message) -> Result<(), Error> {
        if let Some(sender) = self.extensions.get(extension_id) {
            sender.unbounded_send(message).map_err(|e| Error::SendError(e))
        } else {
            Err(Error::RegistryNotFound(format!("Extension {} not found", extension_id)))
        }
    }

    /// Get a stream of all events from all extensions
    pub fn events(&mut self) -> impl Stream<Item = (Id, Response)> + '_ {
        &mut self.event_rx
    }

    /// Unregister an extension
    pub fn unregister(&mut self, extension_id: &Id) -> Result<(), Error> {
        self.extensions
            .remove(extension_id)
            .map(|_| ())
            .ok_or_else(|| Error::RegistryNotFound(format!("Extension {} not found", extension_id)))
    }

    /// Get list of registered extension IDs
    pub fn list_extensions(&self) -> Vec<Id> {
        self.extensions.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_basic() {
        let registry = Registry::new();

        // Would add actual tests here with mock extensions
        assert_eq!(registry.list_extensions().len(), 0);
    }
}
