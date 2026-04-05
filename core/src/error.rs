//! Core error type for the Emporium extension framework.

use serde::{Deserialize, Serialize};

/// Error type for core data operations.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(String),
    /// DataFrame conversion or construction failed.
    #[error("DataFrame error: {0}")]
    DataFrame(String),
    /// Free-form error with a custom message.
    #[error("{0}")]
    Custom(String),
}

impl Error {
    /// Build a custom error from any string-like value.
    pub fn custom<T: Into<String>>(msg: T) -> Self {
        Error::Custom(msg.into())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err.to_string())
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
