<div align="center">

# emporium

[![Crates.io](https://img.shields.io/crates/v/emporium.svg)](https://crates.io/crates/emporium)
[![Documentation](https://docs.rs/emporium/badge.svg)](https://docs.rs/emporium)
[![License](https://img.shields.io/crates/l/emporium.svg)](https://github.com/inboard-ai/emporium/blob/main/LICENSE)

WASM extension framework for Rust applications

</div>

## Quick Start

```rust
use emporium::{Extension, Command, Event};
use futures::StreamExt;

// Load an extension
let mut ext = Extension::load("my-extension-0.1.0.empkg", config).await?;

// Send a command
ext.send(Command::ListTools { correlation_id: None })?;

// Process events
let mut events = ext.events().unwrap();
while let Some(event) = events.next().await {
    // Handle events
}
```

## Extension API

| Method | Description |
|--------|-------------|
| `Extension::load(path, config)` | Load from a `.empkg` file |
| `Extension::from_bytes(bytes, config)` | Load from package bytes |
| `ext.manifest()` | Get full manifest (id, name, version, config_schema, etc.) |
| `ext.id()` / `ext.version()` | Quick accessors |
| `ext.config()` / `ext.set_config(config)` | Read/write extension settings |
| `ext.send(command)` | Send a command to the extension |
| `ext.events()` | Get the event stream (can only be called once) |

## Creating Extensions

Extensions are WASM components that implement the extension interface. See [`core/README.md`](core/README.md) for the full guide.

**Quick overview:**

1. Create a Rust library with `crate-type = ["cdylib"]`
2. Implement the extension interface using `wit-bindgen`
3. Build with `cargo build --target wasm32-wasip2`
4. Package with `cargo emporium package`

## Packaging

Install the CLI and package your extension:

```bash
cargo install --path cli
cargo emporium package path/to/extension
```

This creates a `.empkg` file containing the manifest and WASM component.

## Serving Extensions

Use [shopkeep](https://github.com/inboard-ai/store) to serve extensions over HTTP:

```bash
cargo install shopkeep
shopkeep --registry-path ./extensions
```

## Crates

| Crate | Description |
|-------|-------------|
| `emporium` | Main library for loading and running extensions |
| `emporium-core` | Core types shared between host and extensions |
| `cargo-emporium` | CLI for packaging extensions |

## License

MIT
