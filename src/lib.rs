//! Emporium — WASM extension framework for Rust applications.
//!
//! This crate loads `.empkg`-packaged components against typed WIT provider
//! interfaces. Consumers interact with [`Extension`] — a clone-friendly
//! handle to a worker thread that owns the wasmtime `Store`.
//!
//! # Example
//!
//! ```ignore
//! use emporium::Extension;
//! use serde_json::json;
//!
//! # async fn run() -> Result<(), emporium::Error> {
//! let ext = Extension::load("kv-repl-0.1.0.empkg", json!({})).await?;
//! let tools = ext.list_tools().await?;
//! for tool in &tools {
//!     println!("{}: {}", tool.id, tool.description);
//! }
//! let output = ext.execute_tool("set", json!({"key": "a", "value": "1"})).await?;
//! # Ok(())
//! # }
//! ```

pub mod block;
pub mod data;
pub mod discovery;
pub mod error;
pub mod extension;
pub mod formula;
pub mod host_data;
pub mod manifest;

// Internal modules
mod convert;
mod wasm;

// Re-exports
pub use discovery::{SearchIndex, SearchResult};
pub use emporium_types::{ManifestColumn, ManifestDataSource, ManifestTool};
pub use error::Error;
pub use extension::Extension;
pub use manifest::Manifest;
