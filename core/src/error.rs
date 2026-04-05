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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_wraps_any_stringlike_type() {
        let from_str = Error::custom("boom");
        let from_string = Error::custom(String::from("boom"));
        assert!(matches!(from_str, Error::Custom(ref s) if s == "boom"));
        assert!(matches!(from_string, Error::Custom(ref s) if s == "boom"));
    }

    #[test]
    fn from_serde_json_error_produces_json_variant() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: Error = json_err.into();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn display_includes_message_content() {
        let custom = Error::custom("oops");
        assert!(custom.to_string().contains("oops"));

        let json = Error::Json("bad json".to_string());
        assert!(json.to_string().contains("bad json"));

        let df = Error::DataFrame("bad df".to_string());
        assert!(df.to_string().contains("bad df"));
    }

    #[test]
    fn serde_roundtrip_preserves_variant() {
        let original = Error::custom("hello");
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Error = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, Error::Custom(ref s) if s == "hello"));

        let original = Error::Json("bad".to_string());
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Error = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, Error::Json(ref s) if s == "bad"));

        let original = Error::DataFrame("broken".to_string());
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Error = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded, Error::DataFrame(ref s) if s == "broken"));
    }
}
