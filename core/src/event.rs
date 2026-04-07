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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_event_preserves_percent_and_message() {
        let evt = Event::Progress(Progress {
            percent: Some(42),
            message: "halfway".to_string(),
        });
        match evt {
            Event::Progress(p) => {
                assert_eq!(p.percent, Some(42));
                assert_eq!(p.message, "halfway");
            }
            _ => panic!("expected Progress"),
        }
    }

    #[test]
    fn invalidate_all_variant_constructible() {
        let evt = Event::Invalidate(Invalidate::All);
        assert!(matches!(evt, Event::Invalidate(Invalidate::All)));
    }

    #[test]
    fn tools_changed_variant_constructible() {
        let evt = Event::ToolsChanged;
        assert!(matches!(evt, Event::ToolsChanged));
    }

    #[test]
    fn invalidate_resource_and_tool_named_arms_match() {
        let resource_evt = Event::Invalidate(Invalidate::Resource("res1".to_string()));
        let tool_evt = Event::Invalidate(Invalidate::Tool("tool1".to_string()));

        // Documents the downstream-crate pattern of using a named catch-all
        // `other =>` arm. Within this crate `#[non_exhaustive]` doesn't make
        // the match incomplete, so clippy flags the arm as unreachable —
        // allow it to keep the pattern visible.
        #[allow(unreachable_patterns)]
        let resource_desc = match &resource_evt {
            Event::Invalidate(Invalidate::Resource(id)) => format!("resource:{id}"),
            Event::Invalidate(Invalidate::Tool(name)) => format!("tool:{name}"),
            Event::Invalidate(Invalidate::All) => "all".to_string(),
            Event::Progress(_) => "progress".to_string(),
            Event::ToolsChanged => "tools-changed".to_string(),
            other => {
                let _ = other;
                "unknown".to_string()
            }
        };
        assert_eq!(resource_desc, "resource:res1");

        #[allow(unreachable_patterns)]
        let tool_desc = match &tool_evt {
            Event::Invalidate(Invalidate::Resource(id)) => format!("resource:{id}"),
            Event::Invalidate(Invalidate::Tool(name)) => format!("tool:{name}"),
            Event::Invalidate(Invalidate::All) => "all".to_string(),
            Event::Progress(_) => "progress".to_string(),
            Event::ToolsChanged => "tools-changed".to_string(),
            other => {
                let _ = other;
                "unknown".to_string()
            }
        };
        assert_eq!(tool_desc, "tool:tool1");
    }
}
