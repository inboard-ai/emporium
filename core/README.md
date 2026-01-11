<div align="center">

# emporium-core

[![Crates.io](https://img.shields.io/crates/v/emporium-core.svg)](https://crates.io/crates/emporium-core)
[![Documentation](https://docs.rs/emporium-core/badge.svg)](https://docs.rs/emporium-core)
[![License](https://img.shields.io/crates/l/emporium-core.svg)](https://github.com/inboard-ai/emporium/blob/main/LICENSE)

Core types for the emporium extension framework

</div>

## Overview

This crate provides the shared types used by both the host application and extensions:

- `Command` - Commands sent to extensions (ListTools, ExecuteTool, etc.)
- `Response` - Responses from extensions (ToolList, ToolResult, Error, etc.)
- `ToolInfo` - Tool metadata and JSON schema
- `ToolResult` - Results from tool execution (Text, DataFrame)

## Creating Extensions

Extensions are WASM components that implement the extension interface defined in `wit/extension.wit`.

### 1. Project Setup

Create a new Rust library:

```bash
cargo new --lib my-extension
cd my-extension
```

Update `Cargo.toml`:

```toml
[package]
name = "my-extension"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
emporium-core = { git = "https://github.com/inboard-ai/emporium.git" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen = "0.35"
```

### 2. Generate Bindings

Copy the `wit/` folder from emporium to your extension, then generate bindings in `src/lib.rs`:

```rust
wit_bindgen::generate!({
    path: "./wit",
    world: "extension-world",
});

use emporium::extensions::extension::{Guest, GuestInstance, Metadata};
use emporium::extensions::types::Metadata as MetadataRecord;
```

### 3. Implement the Interface

```rust
struct MyExtension;

impl Guest for MyExtension {
    fn get_metadata() -> MetadataRecord {
        MetadataRecord {
            id: "my-extension".to_string(),
            name: "My Extension".to_string(),
            version: "0.1.0".to_string(),
            description: "A sample extension".to_string(),
        }
    }
}

struct Instance {
    config: serde_json::Value,
}

impl GuestInstance for Instance {
    fn new(config: String) -> Self {
        let config: serde_json::Value = serde_json::from_str(&config)
            .unwrap_or(serde_json::json!({}));
        Self { config }
    }

    fn update(&self, command: String) -> Result<String, String> {
        // Parse command as emporium_core::Command
        let cmd: emporium_core::Command = serde_json::from_str(&command)
            .map_err(|e| e.to_string())?;

        // Handle command and return Response as JSON
        let response = match cmd {
            emporium_core::Command::ListTools { correlation_id } => {
                emporium_core::Response::ToolList {
                    tools: vec![],  // Your tools here
                    correlation_id,
                }
            }
            // Handle other commands...
            _ => emporium_core::Response::Error {
                message: "Unknown command".to_string(),
                correlation_id: None,
            }
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn view(&self) -> String {
        serde_json::to_string(&self.config).unwrap_or_default()
    }
}

export!(MyExtension);
```

### 4. Create Manifest

Create `manifest.toml`:

```toml
[extension]
id = "my-extension"
name = "My Extension"
version = "0.1.0"
description = "A sample extension"
author = "Your Name"
license = "MIT"

[component]
entry = "my_extension.wasm"

[config]
schema = '{}'
```

### 5. Build and Package

```bash
# Build the WASM component
cargo build --release --target wasm32-wasip2

# Copy to project root
cp target/wasm32-wasip2/release/my_extension.wasm .

# Package (from emporium repo)
cargo emporium package /path/to/my-extension
```

This creates `my-extension-0.1.0.empkg`.

## Commands and Responses

### Commands

| Command | Description |
|---------|-------------|
| `ListTools` | Request list of available tools |
| `GetToolDetails { tool_id }` | Get schema for a specific tool |
| `ExecuteTool { name, params }` | Execute a tool |
| `Custom { command }` | Extension-specific command |

### Responses

| Response | Description |
|----------|-------------|
| `Metadata` | Extension info (sent automatically on load) |
| `ToolList { tools }` | List of available tools |
| `ToolDetails { tool_id, tool_info }` | Tool schema |
| `ToolResult { name, result }` | Result from tool execution |
| `Error { message }` | Error response |

## License

MIT
