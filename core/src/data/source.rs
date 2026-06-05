//! A data source's identity.
//!
//! [`Id`] is the stable id a `data-provider` extension declares for one of its
//! sources. A newtype so it cannot be swapped with an `extension::Id` or a raw
//! string at a boundary — the [`column::Name`](crate::column::Name) precedent
//! (see `d:column-identity-newtypes`).

use std::fmt;

use serde::{Deserialize, Serialize};

/// The stable identifier of a data source within its extension. Serializes
/// transparently as a bare string, so the WIT / JSON / persisted-binding wire is
/// unchanged — the distinction is a host-language invariant, not a wire one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    /// Wrap a string as a data-source id.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow as a string slice — the only read accessor, so the inner `String`
    /// can't be moved into the wrong slot by field access.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Id {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Id {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
