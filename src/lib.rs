//! Emporium - Extension framework for Rust applications.
//!
//! This crate provides a clean abstraction for loading and communicating with
//! extensions packaged as `.empkg` files. All WASM/runtime details are hidden.
//!
//! # Example
//!
//! ```ignore
//! use emporium::{Extension, Command};
//!
//! let ext = Extension::load("polygon-0.1.0.empkg", config).await?;
//! ext.send(Command::ListTools { correlation_id: None })?;
//!
//! while let Some(event) = ext.events().next().await {
//!     // Handle events
//! }
//! ```

pub mod data;
pub mod error;
pub mod extension;
pub mod manifest;

// Internal modules
mod wasm;
mod wit_check;

// Re-exports
pub use data::{Command, Event};
pub use emporium_types::ManifestTool;
pub use error::Error;
pub use extension::Extension;
pub use manifest::Manifest;
