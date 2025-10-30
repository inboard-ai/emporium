use futures::channel::mpsc;

// Re-export all core types
pub use emporium_core as core;
pub use emporium_core::{ColumnDef, Command, CoreError, Id, Response, Schema, ToolInfo, ToolResult};

/// Extended Response type with emporium-specific variants
#[derive(Debug, Clone)]
pub enum Event {
    /// Connection established with command sender
    Connected(mpsc::UnboundedSender<Command>),

    /// Core response from extension
    Core(emporium_core::Response),
}
