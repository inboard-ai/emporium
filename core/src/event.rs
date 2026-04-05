//! Host-side runtime events broadcast to subscribers.

/// An event emitted by the host to notify subscribers of extension-side changes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// An extension is reporting progress on a long-running operation.
    Progress(Progress),
    /// The set of available tools has changed; clients should re-list.
    ToolsChanged,
    /// A cached resource should be invalidated.
    Invalidate(Invalidate),
}

/// Progress reported by an extension for a long-running operation.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Optional completion percentage in the range 0..=100.
    pub percent: Option<u8>,
    /// Human-readable status message.
    pub message: String,
}

/// Scope of a cache invalidation request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Invalidate {
    /// Invalidate a specific named resource.
    Resource(String),
    /// Invalidate cached results for a specific tool id.
    Tool(String),
    /// Invalidate everything the host has cached for this extension.
    All,
}
