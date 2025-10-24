//! Error types

use crate::Message;
use futures::channel::mpsc::TrySendError;
use std::sync::Arc;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(Arc<std::io::Error>),
    #[error("Wasm error: {0}")]
    Wasm(Arc<wasmtime::Error>),
    #[error("NotFound: {0}")]
    RegistryNotFound(String),
    #[error("AlreadyExists: {0}")]
    RegistryAlreadyExists(String),
    #[error("SendError: {0}")]
    SendError(TrySendError<Message>),
    #[error("Extension not found: {0}")]
    ExtensionNotFound(String),
    #[error("Extension load error: {0}")]
    ExtensionLoadError(String),
    #[error("Manifest error: {0}")]
    ManifestError(ManifestError),
    #[error("{0}")]
    Custom(String),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ManifestError {
    #[error("Manifest read error: {0}")]
    ReadError(String),
    #[error("Missing {0}: {1}")]
    Missing(String, String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(Arc::new(err))
    }
}

impl From<wasmtime::Error> for Error {
    fn from(err: wasmtime::Error) -> Self {
        Error::Wasm(Arc::new(err))
    }
}

impl From<ManifestError> for Error {
    fn from(err: ManifestError) -> Self {
        Error::ManifestError(err)
    }
}

impl From<TrySendError<Message>> for Error {
    fn from(err: TrySendError<Message>) -> Self {
        Error::SendError(err)
    }
}

impl From<String> for Error {
    fn from(err: String) -> Self {
        Error::Custom(err)
    }
}

impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Error::Custom(err.to_string())
    }
}
