//! Operation-specific data associated with a plan step (Phase 3 stub).

use serde::{Deserialize, Serialize};

/// Data payload attached to a plan step.
///
/// This is a Phase 2 stub; Phase 3 fleshes out the concrete variants. It is
/// defined here so that downstream modules can reference `plan::Data` today.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Data {}
