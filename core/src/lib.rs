//! Core data types for the Emporium extension framework.

pub mod block;
pub mod column;
pub mod error;
pub mod event;
pub mod formula;
pub mod operation;
pub mod plan;
pub mod tool;

// Keep this re-export (exists in current code, tests and host use it).
pub use emporium_types::ManifestTool;

/// Convenient alias for the crate's error type. Call sites use module paths
/// for domain types (`tool::Info`, `column::Def`, etc.) rather than re-exports
/// with composite names.
pub use error::Error;
