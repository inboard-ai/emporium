# kv-repl

Reference implementation of an Emporium extension and REPL host. The `kv-extension` WASM component implements all four provider interfaces (`tool-provider`, `block-provider`, `formula-provider`, and `host-data` imports) against the `full-extension` world. The `kv-repl` binary loads it and provides an interactive REPL that exercises every capability.

## kv-extension

An in-memory key-value store packaged as a WASM component (`crate-type = ["cdylib"]`) using `wit-bindgen`.

### Tools (7)

| Tool | Description | Output |
|------|-------------|--------|
| `get` | Get the value for a key | Text |
| `set` | Set a key to a value | Text |
| `delete` | Delete a key | Text |
| `get_all` | List all key-value pairs | Text |
| `clear` | Clear all entries | Text |
| `stats` | Get store statistics | Text |
| `list_keys` | List all keys as a single-column DataFrame | DataFrame (`key` column) |

### Block type (1)

`prefix-tracker` -- tracks a set of key prefixes and supports five operations:

| Operation | Outcome | Description |
|-----------|---------|-------------|
| `add_prefix` | `StateUpdate` | Add a prefix to the tracked set |
| `query` | `Query` | Build a query plan filtering keys by tracked prefixes |
| `rename_prefix` | `StateUpdateWithQuery` | Rename a prefix and produce a new query plan |
| `analyze` | `Computed` | Stream `kv-keys` via a host-data cursor, tally character frequencies |
| `add_and_analyze` | `StateUpdateWithComputed` | Add a prefix and re-analyze in one step |

### Formulas (3)

| Formula | Args | Description |
|---------|------|-------------|
| `KV_COUNT` | (none) | Number of entries in the KV store |
| `KV_GET` | `[key]` | Value for the given key, or null if absent |
| `KV_EXISTS` | `[key]` | Whether the given key is present |

### Host-data usage

The `analyze` and `add_and_analyze` operations open a cursor on the `kv-keys` host resource and stream rows in batches of 64, computing character frequencies across all keys.

## REPL commands

| Command | Description |
|---------|-------------|
| `get <key>` | Get a value |
| `set <key> <value>` | Set a value |
| `delete <key>` | Delete a key |
| `list` | List all key-value pairs |
| `clear` | Clear all entries |
| `stats` | Show store statistics |
| `tools` | List available tools |
| `view` | Print the extension's rendered view (JSON dump of KV state) |
| `block-types` | List block kinds advertised by the extension |
| `block-create <prefix>...` | Create a prefix-tracker block with initial prefixes |
| `block-add <prefix>` | Add a prefix to the current block state |
| `block-rename <old> <new>` | Rename a prefix and run the follow-up query plan |
| `block-query` | Run the current block's query plan against KV keys |
| `sync-keys` | Mirror KV keys into the `kv-keys` host-data resource |
| `analyze` | Run character-frequency analysis over `kv-keys` via host cursor |
| `add-analyze <prefix>` | Add prefix and re-analyze in one step |
| `formulas` | List formulas advertised by the extension |
| `formula <name> [args...]` | Evaluate a formula with positional arguments |
| `help` | Print help message |
| `quit` | Exit the REPL |

## Running

```bash
cargo run -p kv-repl
```

The `build.rs` script automatically compiles the `kv-extension` to a WASM component and packages it as an `.empkg` before the host binary is linked.

Enable verbose event logging (prints `host-events` notifications as they arrive):

```bash
EMPORIUM_REPL_VERBOSE=1 cargo run -p kv-repl
```

## Example session

```
$ cargo run -p kv-repl
Loading extension from: .../kv-extension-0.1.0.empkg
Loaded: kv v0.1.0
A simple in-memory key-value store

kv> set foo bar
OK
kv> set baz qux
OK
kv> set foobar 42
OK
kv> stats
3 entries
kv> list
baz: qux
foo: bar
foobar: 42
kv> sync-keys
Synced 3 keys to kv-keys resource
kv> block-create f b
Block created. State: {"prefixes":["f","b"]}
kv> block-query
Query plan matched:
baz
foo
foobar
kv> analyze
Character frequencies:
  'a': 2
  'b': 2
  'f': 2
  'o': 3
  'r': 1
  'z': 1
kv> formula KV_COUNT
3
kv> formula KV_GET foo
"bar"
kv> formula KV_EXISTS missing
false
kv> quit
Goodbye!
```

## Structure

```
kv-repl/
  src/main.rs              REPL host -- loads extension, dispatches commands
  build.rs                 Builds the extension WASM component and packages .empkg
  Cargo.toml               Host binary dependencies
  extension/
    src/lib.rs             Extension implementation (full-extension world)
    wit/extension.wit      WIT interface definition (copied from workspace root)
    manifest.toml          Extension metadata for packaging
    Cargo.toml             WASM component dependencies (wit-bindgen, serde_json)
```
