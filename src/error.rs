//! Error types for the host extension framework.

use std::sync::Arc;

/// Error type surfaced by the `emporium` host crate.
///
/// Variants cover package extraction, manifest parsing, worker-thread
/// lifecycle, world validation, and underlying WASM/IO failures.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Underlying I/O failure (package reads, archive extraction, etc.).
    #[error("I/O error: {0}")]
    Io(Arc<std::io::Error>),

    /// A wasmtime API call failed (component compilation, instantiation, call).
    #[error("Wasm error: {0}")]
    Wasm(Arc<wasmtime::Error>),

    /// The named extension could not be resolved in a registry.
    #[error("NotFound: {0}")]
    RegistryNotFound(String),

    /// An entry with this identifier already exists in a registry.
    #[error("AlreadyExists: {0}")]
    RegistryAlreadyExists(String),

    /// Failed to send a request to the worker thread (receiver closed).
    #[error("SendError: the worker receiver has been dropped")]
    SendError,

    /// The named extension could not be found on disk.
    #[error("Extension not found: {0}")]
    ExtensionNotFound(String),

    /// An extension failed to load for an unspecified reason.
    #[error("Extension load error: {0}")]
    ExtensionLoadError(String),

    /// The extension returned an error from a typed call.
    #[error("Extension error: {0}")]
    ExtensionError(String),

    /// The extension's block-provider returned an error from a typed call.
    #[error("Block-provider error: {0}")]
    BlockOperation(String),

    /// The extension's formula-provider returned an error from a typed call.
    #[error("Formula-provider error: {0}")]
    FormulaOperation(String),

    /// The extension manifest was invalid.
    #[error("Manifest error: {0}")]
    ManifestError(ManifestError),

    /// JSON (de)serialization failed.
    #[error("JSON error: {0}")]
    JsonError(Arc<serde_json::Error>),

    /// The worker thread panicked before it could report its metadata.
    #[error("worker thread died during extension init")]
    ThreadDiedDuringInit,

    /// The worker thread has exited or panicked; further requests are
    /// impossible.
    #[error("extension worker has crashed")]
    ExtensionCrashed,

    /// The component exports interfaces the declared world does not cover.
    #[error("world {declared}: component exports interfaces not declared: {extra:?}")]
    WorldExtraExports {
        /// The world name declared by the manifest.
        declared: String,
        /// Fully-qualified interface names present but not declared.
        extra: Vec<String>,
    },

    /// The component is missing interfaces its declared world requires.
    #[error("world {declared}: component missing declared interfaces: {missing:?}")]
    WorldMissingExports {
        /// The world name declared by the manifest.
        declared: String,
        /// Fully-qualified interface names required but absent.
        missing: Vec<String>,
    },

    /// The manifest declared a world this host does not yet support.
    #[error("unsupported world: {0}")]
    UnsupportedWorld(String),

    /// Free-form, last-resort error message.
    #[error("{0}")]
    Custom(String),
}

/// Manifest-parsing failure details.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// The manifest file could not be read from the archive.
    #[error("Manifest read error: {0}")]
    ReadError(String),

    /// A required manifest field is missing.
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

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::JsonError(Arc::new(err))
    }
}

impl From<emporium_core::Error> for Error {
    fn from(err: emporium_core::Error) -> Self {
        Error::Custom(err.to_string())
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
