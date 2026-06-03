# kv-js — a JavaScript Emporium extension

Proof that the host↔extension boundary is the **WIT/ABI contract, not Rust**.
This is a key-value extension written in plain JavaScript, compiled to a wasm
component with [`jco`](https://github.com/bytecodealliance/jco). It exports the
exact same `emporium:extensions@0.3.0` `tool-extension` world the Rust extensions
do, so the host loads and drives it identically.

`list_keys` returns a data frame whose **Arrow IPC buffer is produced by the
pure-JavaScript [`apache-arrow`](https://www.npmjs.com/package/apache-arrow)
library** — so even the columnar data wire is written by a non-Rust stack and
read by the host's polars decoder. (Python can't do this in-component, because
its Arrow libraries — pyarrow, polars — are native modules that don't load in a
wasm sandbox; JS's `apache-arrow` is pure JS.)

## Build

```sh
npm install
node_modules/.bin/esbuild kv.js --bundle --format=esm --platform=browser --outfile=kv.bundled.js
node_modules/.bin/jco componentize kv.bundled.js --wit wit/ --world-name tool-extension -o kv_js.wasm
tar czf kv-js.empkg manifest.toml kv_js.wasm
```

(The `.wasm`, `.empkg`, bundle, and `node_modules/` are git-ignored — rebuild
with the above.)

## Run

From `crates/emporium`, with the generic harness:

```sh
cargo run -p run-ext --release -- examples/kv-js/kv-js.empkg
```

Expected: the host loads `kv-js`, lists its tools, round-trips `set`/`get`, and
decodes the JS-built Arrow frame from `list_keys`:

```
decoded 3 row(s) x 1 col(s) from the JS-built Arrow buffer:
  {"Key":"bar"}
  {"Key":"baz"}
  {"Key":"foo"}
```
