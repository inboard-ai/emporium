//! Output shape for a data source — what the host knows about the result
//! before fetch.

use serde::{Deserialize, Serialize};

use crate::column;

/// What the host knows about output shape before fetch — the keystone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    /// Statically known regardless of input; the host seeds schema immediately.
    Known(Shape),
    /// Knowable only after the selection is made; the host calls
    /// `describe(source, selection)` once the user has chosen. v1b.
    Resolved,
}

/// The shape of a source's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Shape {
    /// Tabular relation with a known column set.
    Rows(Vec<column::Def>),
    /// A single headline value.
    Scalar(column::Def),
    /// A complete resource the host references without reshaping. v2.
    Finished,
}
