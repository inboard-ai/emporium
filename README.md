<div align="center">

# emporium

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/inboard-ai/emporium/blob/main/LICENSE)

WASM extension framework for Rust applications

</div>

## Quick Start

```rust
use emporium::{Extension, Command, Event};
use futures::StreamExt;

// Load an extension
let mut ext = Extension::load("sql-0.1.0.empkg", config).await?;

// Send a command
ext.send(Command::ListTools { correlation_id: None })?;

// Process events
let mut events = ext.events().unwrap();
while let Some(event) = events.next().await {
    match event {
        Event::Response(response) => { /* handle response */ }
        Event::Log { level, message } => { /* extension log output */ }
    }
}
```

## Extension API

| Method | Description |
|--------|-------------|
| `Extension::load(path, config)` | Load from a `.empkg` file |
| `Extension::from_bytes(bytes, config)` | Load from package bytes |
| `ext.manifest()` | Get full manifest (id, name, version, tools, config_schema, etc.) |
| `ext.id()` / `ext.version()` | Quick accessors |
| `ext.config()` / `ext.set_config(config)` | Read/write extension settings |
| `ext.send(command)` | Send a command to the extension |
| `ext.sender()` | Get a cloneable command sender |
| `ext.events()` | Get the event stream (can only be called once) |

## Commands and Responses

Commands sent to extensions:

```rust
Command::ListTools { correlation_id }
Command::GetToolDetails { tool_id, correlation_id }
Command::ExecuteTool { name, params, correlation_id }
Command::Custom { command, correlation_id }
```

Responses from extensions:

```rust
Response::ToolList { tools, correlation_id }
Response::ToolDetails { tool_id, tool_info, correlation_id }
Response::ToolResult { name, result: Result<ToolResult, _>, correlation_id }
Response::Error { message, correlation_id }
```

Tool results are either `ToolResult::Text` or `ToolResult::DataFrame`, each with optional `label`, `source` (e.g. the SQL query that produced it), and a per-result `store` flag to override the tool's `cacheable` default.

## Creating Extensions

Extensions are WASM components that implement the extension WIT interface. See [`core/README.md`](core/README.md) for the full guide.

1. Create a Rust library with `crate-type = ["cdylib"]`
2. Implement the extension interface using `wit-bindgen`
3. Build with `cargo emporium build`
4. Package with `cargo emporium package`

## Packaging

Install the CLI:

```bash
cargo install --path cli
```

Build and package an extension:

```bash
cargo emporium build              # compile to WASM
cargo emporium package            # create .empkg archive
cargo emporium validate           # check manifest + WASM
cargo emporium check -r <url>     # check if version exists in registry
```

The `.empkg` format is a gzipped tarball containing `manifest.toml`, the WASM binary, and optional README/icon/license files.

## Serving Extensions

Extensions are served by [shopkeep](https://github.com/inboard-ai/shopkeep), a Cloudflare Worker backed by R2. Publish with the `cargo-shopkeep` CLI:

```bash
cargo shopkeep publish --registry https://extensions.inboard.ai
```

See the [shopkeep repo](https://github.com/inboard-ai/shopkeep) for the full registry API and CLI docs.

## Crates

| Crate | Description |
|-------|-------------|
| `emporium` | Main library — load extensions, send commands, receive events |
| `emporium-core` | Shared types — `Command`, `Response`, `ToolResult`, `ToolInfo` |
| `emporium-types` | Lightweight types for downstream crates (`ManifestTool`) |
| `cargo-emporium` | CLI — build, package, validate, check |

## License

MIT
