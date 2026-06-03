# wasm-boundary-bench

Answers one question with real numbers: **when a wasm extension returns a table
of rows to its host, should it serialize them as a JSON string or as an Arrow IPC
buffer?**

The `databench` guest bakes a sample of the `diamonds` dataset and exports the
same `n` rows two ways. This harness loads it as a genuine wasm **component**
through wasmtime (the same loader `emporium/src/wasm.rs` uses) and times each
wire across the live component boundary. Because a real component is loaded, the
measurement includes the costs a host actually pays per tool call: the **in-guest
(wasm) encode** and the **copy of the result out of guest linear memory**.

It dogfoods the real toolchain end to end — `cargo component` builds the guest (a
large-dataset variation of the kv example extension), a `wasmtime` + WASI p2
linker loads it, and the `emporium-core` codec decodes on the host side.

## Why a bench-specific WIT

The canonical emporium `tool-provider` has **no binary channel** — every payload
is a `string` — so Arrow bytes can't cross it without a breaking change to all 17
extensions (that work is roadmap step R1b). To measure both wires *now* without
that migration, the guest exports a tiny purpose-built world
(`extension/wit/databench.wit`) with two functions returning the same rows as
`string` vs `list<u8>`. Everything else — build flow, component model, WASI
linking, the codec — is the production machinery.

## What it does

1. `build.rs` runs `cargo component build` on the `databench` guest, which bakes a
   5,000-row sample of the seaborn `diamonds` dataset (10 CSV columns + 1 derived
   `score` column = 7 float, 1 int, 3 string) and cycles it to reach any `n`.
2. The host loads the `.wasm` component (component model + WASI p2 linker).
3. It crosses the boundary both ways at `n` rows: `rows-json` (JSON string) and
   `rows-arrow` (Arrow IPC, encoded in-guest with arrow-rs).
4. **Value fidelity:** the Arrow wire carries the source f64 bits verbatim, so it's
   ground truth. The host compares the JSON-decoded frame against it per column,
   bit-for-bit, and reports any drift (see "Does JSON corrupt data?" below).
5. It times **encode + transfer** (guest-side encode + the boundary copy) and
   **host decode** separately, and reports wire size.

## Run it

```sh
cargo run -p wasm-boundary-bench --release            # 50k rows, 20 iters
cargo run -p wasm-boundary-bench --release -- 200000  # 200k rows
cargo run -p wasm-boundary-bench --release -- 5000 50 # one fixture pass
```

First build takes ~90s (compiling the guest's arrow-rs stack to wasm and the
host's wasmtime + polars). Re-runs are instant.

```
┌────────────────────────────────────────────────────────────────┐
│ data-frame wire: JSON string vs Arrow IPC · n=50000 · 20 iters │
└────────────────────────────────────────────────────────────────┘

value fidelity vs the bit-exact Arrow wire (50000 rows):
  arrow   exact by construction — carries the source f64 bits
  json    3280 float cell(s) corrupted by ≤1 ULP (serde_json's default parser):
            score   3280/50000 rows

                              JSON         Arrow    advantage
  wire size              7006091 B     4385690 B    1.60× smaller
  encode + transfer        91235 µs       2491 µs   36.6× faster
  decode (host)            74393 µs        804 µs   92.6× faster
  total                   165628 µs       3295 µs   50.3× faster
```

The metrics:

- **wire size** — bytes on the wire for each format.
- **encode + transfer** — the extension serializes `n` rows inside wasm, then the
  bytes are copied out of guest memory across the component boundary.
- **decode (host)** — the host parses those bytes into an in-memory table (polars).
- **total** — encode + transfer + decode: the end-to-end cost a host pays to pull
  one tool result.

## Results

Consistent across scale (Apple Silicon, release build):

| n | wire size | encode + transfer | host decode | total | json float drift |
|---|---|---|---|---|---|
| 5,000 | 1.59× smaller | 34× faster | 57× faster | **41× faster** | 328 cells |
| 50,000 | 1.60× smaller | 37× faster | 93× faster | **50× faster** | 3,280 cells |
| 200,000 | 1.60× smaller | 38× faster | 109× faster | **56× faster** | 13,120 cells |

Arrow wins on every axis, the end-to-end gap widens with row count as decode comes
to dominate, and the JSON wire silently corrupts ~6.6% of the computed column's
cells (next section).

## Why the encode win is the surprising part

A naive measurement done **in-process** (encoding both wires with native
`serde_json` and `arrow-rs`, no wasm) shows Arrow encode roughly *tied with or
slightly slower than* JSON — native `serde_json` is fast. That result is
misleading, because extensions don't encode natively; they encode **inside
wasm**. Across the real boundary, Arrow encode is **~35× faster**, for two
compounding reasons:

1. **In-wasm `serde_json` is brutally slow.** Building 50k×10 JSON objects in the
   sandbox — half a million field writes and allocations under the wasm
   allocator, no host SIMD — costs ~78 ms. arrow-rs fills columnar buffers from
   typed vectors in ~2 ms.
2. **A bigger string is a bigger boundary copy.** JSON is 1.55× larger, so the
   bytes copied out of guest memory are 1.55× heavier too.

The lesson: the encode cost is the half you can only see in the real environment,
and it's the half that most favors Arrow.

## Does JSON corrupt data? Yes — on computed columns

The fidelity check answers a question the performance numbers don't: **are the
values even correct?** The diamonds CSV columns survive the round-trip intact, so
a wire-vs-wire check would report "all good." The derived `score` column tells the
real story: **the JSON wire corrupts ~6.6% of its cells by 1 ULP**, Arrow none.

The mechanism, precisely:

- JSON *serialization* is fine — serde_json uses ryu, which emits the shortest
  decimal string that round-trips.
- JSON *deserialization* is the culprit. serde_json's **default** float parser is
  fast but not correctly rounded; on ~6–9% of full-precision values it returns
  the f64 one ULP from the intended one. (Measured directly: of 200k computed
  values, ~17.5k drift through `to_string` → `from_str`.)
- serde_json's **`float_roundtrip`** feature makes the parser correctly rounded —
  drift drops to 0/200k. **The inboard host uses `serde_json` without that
  feature,** so its JSON-rows path corrupts computed floats today.
- Arrow carries the raw f64 bytes, so it's exact regardless of parser config.

Why only `score` drifts: CSV values like `61.5` or `0.23` are short decimals whose
nearest f64 the fast parser nails. `score = price*0.37 + carat*100` produces values
like `120.62000000000001` whose long shortest-decimal the fast parser rounds
wrong. Extensions return computed and aggregated columns routinely — this is not a
corner case.

**Takeaway:** moving the wire to Arrow fixes float fidelity for free. Staying on
JSON requires at minimum `serde_json/float_roundtrip` on the host — and still
carries the encode/size/speed costs above.

## Lessons learned

**1. Measure in the deployment environment.** The encode comparison flips from
"roughly tied" to "35× for Arrow" purely by moving from native to in-wasm. A
performance claim made in the wrong environment is worse than none.

**2. Check against ground truth, not wire-vs-wire.** The first cut of this bench
compared the two wires to each other; against passthrough data they agree, so it
looked clean and hid the bug. Using the bit-exact Arrow wire as ground truth is
what surfaced the JSON parser corrupting the computed column.

**3. arrow-rs targets `wasm32-wasip1` cleanly.** `arrow-array` + `arrow-ipc` +
`arrow-schema` with `default-features = false` compile to a component with no
fuss — Arrow encode in the guest needs no polars in the sandbox.

**4. Match the IPC sub-format end to end.** arrow-rs `FileWriter` writes the Arrow
IPC **file** format; polars `IpcReader` reads the **file** format. The *stream*
format (`StreamWriter` / `IpcStreamReader`) is a different framing; mixing the two
fails at decode. The guest writes file format; the host reads file format.

**5. String-heavy data narrows the size gap, not the speed gap.** diamonds is 30%
string columns, where JSON and Arrow cost about the same per value, so the size
win is 1.55× rather than the ~1.8× of a mostly-numeric table. The speed win is
unaffected — it comes from eliminating the parse/encode, not from the byte count.

## Scope & caveats

- The fixture is a 5,000-row **sample** of diamonds, cycled to reach larger `n`:
  real column shapes and type mix, but row identity repeats past 5k. Fine for
  throughput; not for cardinality-sensitive work.
- Timing is coarse averaged wall-clock, not a statistical benchmark with variance.
- The guest encodes from pre-parsed typed columns. A real extension first parses
  an API/DB response into those columns — a cost both wires share, so it's
  excluded here.

## Related

- Decision: `d:extension-payload-strategy`
- Roadmap: `../../docs/extension-data-transport-roadmap.md` (R1 validation; R1c is the guest-side arrow-rs encode)
- Codec under test: `emporium_core::tool::data_frame::{to_arrow_ipc, from_arrow_ipc}`
