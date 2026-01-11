# kv-repl

An interactive REPL that demonstrates how to build and use an Emporium extension.

## Prerequisites

Install the required cargo extensions:

```bash
cargo install cargo-component
cargo install cargo-emporium # or: cargo install --path ../../cli 
```

## Running

```bash
cargo run -p kv-repl
```

The build script automatically compiles and packages the KV extension before running.

## Usage

```
kv> set mykey hello
OK
kv> get mykey
hello
kv> list
mykey: hello
kv> stats
1 entry
kv> delete mykey
OK
kv> quit
Goodbye!
```

## Structure

```
kv-repl/
├── src/main.rs       # REPL that loads and interacts with the extension
├── build.rs          # Builds the extension automatically
└── extension/        # The KV extension (a separate WASM component)
    ├── src/lib.rs    # Extension implementation
    ├── wit/          # WIT interface definition
    ├── manifest.toml # Extension metadata
    └── Cargo.toml
```

## How it works

1. `build.rs` runs `cargo component build` to compile the extension to WASM
2. `build.rs` runs `cargo emporium package` to create the `.empkg` file
3. The REPL loads the packaged extension and sends commands to it
4. The extension processes commands and returns responses

## Writing your own extension

Use `extension/` as a template. Your `Cargo.toml` should depend on `emporium-core`:

```toml
[dependencies]
emporium-core = { git = "https://github.com/inboard-ai/emporium.git" }
```

The `[patch]` section in this example is only needed when developing against a local emporium checkout.
