# emporium

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/inboard-ai/emporium/blob/main/LICENSE)

WASM extension framework for Rust. A host application loads extensions as WASM components, exposing their tools, blocks, and formulas to an AI agent through typed WIT interfaces.

## Architecture

Extensions are WASM components that export one or more provider interfaces defined in `wit/extension.wit`. The host loads a `.empkg` package (gzipped tarball containing `manifest.toml` + compiled WASM), instantiates the component via wasmtime, and communicates through strongly-typed WIT bindings -- no string-serialized command/response protocol.

### WIT interfaces

**Exported by the extension (extension -> host):**

| Interface | Purpose |
|-----------|---------|
| `types` | Shared record types (`column-def`) used across interfaces |
| `extension` | Identity and lifecycle: `get-metadata`, `init`, `view` |
| `tool-provider` | Data tools: `list-tools`, `execute-tool` returning text or DataFrame outputs |
| `block-provider` | Stateful block types with operations: `create`, `plan`, `validate` |
| `formula-provider` | Spreadsheet-style formulas: `get-formulas`, `evaluate` |

**Imported by the extension (host -> extension):**

| Interface | Purpose |
|-----------|---------|
| `host-data` | Cursor-based streaming access to host-managed resources (`cursor.open`, `next-batch`, `get-schema`, `row-count`) |
| `host-events` | Push notifications from extension to host: progress, invalidation, tools-changed |

### Worlds (additive tiers)

Each world includes the previous, so an extension picks the tier matching its capabilities:

| World | Exports | Imports |
|-------|---------|---------|
| `tool-extension` | `extension` + `tool-provider` | `host-events` |
| `block-extension` | + `block-provider` | |
| `rich-extension` | + `formula-provider` | |
| `full-extension` | (all of the above) | + `host-data` |

## Workspace structure

| Crate | Path | Description |
|-------|------|-------------|
| `emporium` | `.` | Host runtime -- loads `.empkg` packages, manages wasmtime worker threads, provides the `Extension` API |
| `emporium-core` | `core/` | Domain types: `tool::Info`, `tool::Output`, `block::TypeInfo`, `plan::Outcome`, `formula::Def`, `event::Event`, `column::Def` |
| `emporium-types` | `types/` | Lightweight manifest types (`ManifestTool`) for downstream crates that don't need the full runtime |
| `cargo-emporium` | `cli/` | CLI tool for building and packaging extensions (`cargo emporium build`, `package`, `validate`, `check`) |
| `kv-extension` | `examples/kv-repl/extension/` | Example WASM extension: in-memory key-value store implementing `full-extension` |
| `kv-repl` | `examples/kv-repl/` | Example REPL host that loads the kv-extension and exercises all provider interfaces |

## Quick start

```rust
use emporium::Extension;
use serde_json::json;

let ext = Extension::load("my-extension-0.1.0.empkg", json!({})).await?;

// List tools
let tools = ext.list_tools().await?;

// Execute a tool
let output = ext.execute_tool("set", json!({"key": "a", "value": "1"})).await?;

// Access block and formula providers (if the extension exports them)
if let Some(block) = ext.block() {
    let types = block.types().await?;
}
if let Some(formula) = ext.formula() {
    let defs = formula.defs().await?;
}
```

The `Extension` type is `Send + Sync + Clone` -- clones share the same worker thread via an `mpsc` channel, and the thread exits when the last clone drops.

For host-data cursor support, use `Extension::load_with_data` and provide a `DataRegistry`:

```rust
use emporium::host_data::DataRegistry;

let registry = DataRegistry::new();
registry.register("resource-id", schema, rows);
let ext = Extension::load_with_data("ext.empkg", json!({}), registry).await?;
```

## Manifest v2 and tool discovery

Extensions declare their capabilities in `manifest.toml`. As of Phase 8, the
manifest requires two additional fields for AI-agent-driven tool discovery:

- **`overview`** (extension-level) — an LLM-facing description of the
  extension's capabilities, written to be keyword-dense and specific.
- **`schema`** (per tool) — a JSON Schema string describing each tool's
  parameters.

Optional but recommended fields include `topics` (searchable tags),
`examples` (sample inputs), `primary` (include tool in the LLM system
prompt), `cacheable`, and `activity` (display hints).

The host exposes a `SearchIndex` (in `src/discovery.rs`) that indexes all
loaded extensions and supports fuzzy tool search via a `search_tools`
meta-tool. Non-primary tools are discovered through this search rather than
being listed upfront.

Run `cargo emporium validate` to check that your manifest conforms to v2.
See [`docs/EXTENSION_MIGRATION_GUIDE.md`](docs/EXTENSION_MIGRATION_GUIDE.md)
for the full field reference and migration checklist.

## Running the example

The `kv-repl` example builds the `kv-extension` WASM component automatically via its `build.rs` and loads it into a REPL host:

```bash
cargo run -p kv-repl
```

Enable verbose event logging:

```bash
EMPORIUM_REPL_VERBOSE=1 cargo run -p kv-repl
```

See [`examples/kv-repl/README.md`](examples/kv-repl/README.md) for details.

## Development

Run the full test suite:

```bash
cargo test --workspace
```

Lint:

```bash
cargo clippy --workspace --all-targets
```

Format:

```bash
cargo fmt --all
```

## License

MIT
