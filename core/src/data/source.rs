//! A data source: identity, input shape, output contract, and cardinality.

pub mod input;
pub mod output;

pub use input::{FreeQuery, Input};
pub use output::{Output, Shape};

use std::fmt;

use serde::{Deserialize, Serialize};

/// One pullable source within a configured extension instance.
///
/// `input` says how the host gathers what [`fetch`](crate) needs; `output` says
/// what the host knows about the result shape *before* fetch — the keystone that
/// lets the measure/axis pickers light up immediately for a [`Known`] schema.
///
/// [`Known`]: Output::Known
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// Stable identifier within the extension.
    pub id: String,
    /// Human-facing name shown in the picker.
    pub display_name: String,
    /// One-line description of what the source returns.
    pub description: String,
    /// How the host gathers fetch inputs.
    pub input: Input,
    /// What the host knows about output shape before fetch.
    pub output: Output,
    /// Row-level cardinality: tabular relation, single observation, or scalar.
    #[serde(default)]
    pub cardinality: Cardinality,
}

/// Row-level cardinality of a data source's output.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Cardinality {
    /// Tabular relation (enables aggregation).
    #[default]
    Rows,
    /// Single observation (blocks aggregation).
    Record,
    /// One headline value.
    Scalar,
}

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
