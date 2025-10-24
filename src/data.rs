use serde::{Deserialize, Serialize};

pub type Id = String;

/// A message sent TO an extension - just an opaque string.
/// The host doesn't interpret the content - extensions are free to use any protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message(pub String);

/// A response received FROM an extension - just an opaque string.
/// The host doesn't interpret the content - extensions are free to use any protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response(pub String);
