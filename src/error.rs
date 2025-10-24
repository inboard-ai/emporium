//! Error types

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
    #[error("SendFailed: {0}")]
    RegistrySendFailed(String),
    #[error("Extension not found: {0}")]
    ExtensionNotFound(String),
    #[error("Extension load error: {0}")]
    ExtensionLoadError(String),
    #[error("Manifest error: {0}")]
    ManifestError(ManifestError),
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
