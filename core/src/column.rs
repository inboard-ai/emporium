//! Column definitions for tabular data.
//!
//! A column carries two distinct string-shaped identities that must never be
//! confused: its [`Name`] — the original key from the data source (provenance /
//! the JSON key rows are decoded under) — and its [`Alias`] — the human-facing
//! display label, which is also the field name written into the Arrow IPC
//! buffer. They are separate newtypes precisely so a caller cannot pass one
//! where the other is expected: the swap is a compile error, not a silent bug a
//! test has to chase (see `d:extension-payload-strategy`; `~/.claude/guides/OPAQUE.md`
//! `smart-constructor-newtype`).

use std::fmt;

use serde::{Deserialize, Serialize};

/// A column's original source name — the key from the data source the row
/// values are decoded under. Distinct from [`Alias`] so the two can never be
/// swapped at a boundary.
///
/// Serializes transparently as a bare string, so the JSON / TOML / WIT wire is
/// unchanged — the distinction is a host-language invariant, not a wire one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(String);

/// A column's display alias — the human-facing label, and the field name the
/// guest SDK writes into the Arrow IPC buffer (so it is the polars storage name
/// the host decodes against). Distinct from [`Name`] by type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Alias(String);

macro_rules! string_newtype {
    ($ty:ident) => {
        impl $ty {
            /// Wrap a string as this column identity.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow as a string slice — the only read accessor, so the inner
            /// `String` can't be moved into the wrong slot by field access.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume into the owned string (e.g. to hand to polars).
            #[must_use]
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<String> for $ty {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $ty {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_newtype!(Name);
string_newtype!(Alias);

/// Definition of a single column in a tabular tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Def {
    /// Original column name from the data source (the row-decode key).
    pub name: Name,
    /// Human-friendly display name — also the Arrow field name.
    pub alias: Alias,
    /// Data type (e.g., "string", "number", "date").
    pub dtype: String,
}

impl Def {
    /// Construct a column def from its three parts. Accepts anything
    /// `Into<Name>`/`Into<Alias>` so call sites read naturally, while the field
    /// types keep the two identities distinct thereafter.
    pub fn new(name: impl Into<Name>, alias: impl Into<Alias>, dtype: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: alias.into(),
            dtype: dtype.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_preserves_fields() {
        let original = Def::new("foo", "Foo", "string");
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: Def = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.alias, original.alias);
        assert_eq!(decoded.dtype, original.dtype);
    }

    #[test]
    fn newtypes_serialize_transparently_as_bare_strings() {
        // The wire must stay a flat record of strings — the newtype is a
        // host-language invariant only.
        let def = Def::new("raw_qty", "Quantity", "integer");
        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["name"], "raw_qty");
        assert_eq!(json["alias"], "Quantity");
        assert_eq!(json["dtype"], "integer");
    }
}
