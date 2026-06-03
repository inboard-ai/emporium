//! Benchmark: how should a wasm extension return a table of rows to its host —
//! as a JSON string, or as an Arrow IPC buffer?
//!
//! The `databench` guest (built by `build.rs`) bakes a sample of the `diamonds`
//! dataset and exports two functions returning the same `n` rows each way. This
//! harness loads it as a real wasm component (wasmtime + WASI, the same loader
//! `emporium/src/wasm.rs` uses) and times each wire across the live component
//! boundary, so the numbers include the in-guest encode and the copy of the
//! result out of guest memory — the costs a host actually pays per tool call.
//!
//! Run: `cargo run -p wasm-boundary-bench --release [rows] [iters]`

use std::time::Instant;

use emporium_core::column;
use emporium_core::tool::DataFrame;
use emporium_core::tool::data_frame::from_arrow_ipc;
use polars_core::frame::DataFrame as PolarsFrame;
use serde_json::Value;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        path: "extension/wit",
        world: "databench",
    });
}
use bindings::Databench;

/// Per-store state: just WASI + a resource table. The databench component
/// imports `wasi:cli`/`io`/`clocks`/`filesystem`/`random` (the wasip1 adapter
/// pulls them in), so the host must satisfy them.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// Average wall-clock of `iters` runs of `f`, in microseconds.
fn time_avg_us(iters: u32, mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_secs_f64() * 1e6 / iters as f64
}

/// Count cells in column `name` whose f64 bits differ between two frames.
/// Non-f64 columns return 0. Used to measure the JSON wire's float corruption
/// against the bit-exact Arrow wire.
fn col_bit_drift(a: &PolarsFrame, b: &PolarsFrame, name: &str) -> usize {
    let (Ok(ca), Ok(cb)) = (a.column(name), b.column(name)) else {
        return 0;
    };
    let (sa, sb) = (ca.as_materialized_series(), cb.as_materialized_series());
    let (Ok(fa), Ok(fb)) = (sa.f64(), sb.f64()) else {
        return 0;
    };
    fa.iter()
        .zip(fb.iter())
        .filter(|pair| matches!(pair, (Some(x), Some(y)) if x.to_bits() != y.to_bits()))
        .count()
}

fn main() -> wasmtime::Result<()> {
    let mut args = std::env::args().skip(1);
    let n: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(50_000);
    let iters: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(20);

    // --- Load the real component (mirrors wasm.rs::prepare_component) --------
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_file(&engine, env!("DATABENCH_WASM"))?;

    let mut linker = Linker::<State>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

    let mut store = Store::new(&engine, State {
        wasi: WasiCtxBuilder::new().inherit_stderr().inherit_stdout().build(),
        table: ResourceTable::new(),
    });

    let bench = Databench::instantiate(&mut store, &component, &linker)?;
    let frames = bench.bench_databench_frames();

    // Schema → column::Def. name == alias so the JSON-built and Arrow-built
    // polars frames carry identical column names and compare equal.
    let defs: Vec<column::Def> = frames
        .call_schema(&mut store)?
        .iter()
        .map(|c| column::Def {
            name: c.name.clone(),
            alias: c.name.clone(),
            dtype: c.dtype.clone(),
        })
        .collect();

    let title = format!("data-frame wire: JSON string vs Arrow IPC · n={n} · {iters} iters");
    let w = title.chars().count();
    println!("┌─{}─┐", "─".repeat(w));
    println!("│ {title} │");
    println!("└─{}─┘\n", "─".repeat(w));

    // --- One crossing each: contract + wire sizes (also warms the parse) -----
    let json_wire = frames.call_rows_json(&mut store, n)?;
    let arrow_wire = frames.call_rows_arrow(&mut store, n)?;
    let json_bytes = json_wire.len();
    let arrow_bytes = arrow_wire.len();

    let json_frame: PolarsFrame = DataFrame::new(defs.clone(), serde_json::from_str::<Value>(&json_wire)?, None)
        .to_dataframe()
        .expect("json → polars");
    let arrow_frame: PolarsFrame = from_arrow_ipc(&arrow_wire).expect("arrow → polars");

    assert_eq!(json_frame.height(), n as usize, "row count wrong");
    assert_eq!(
        json_frame.width(),
        arrow_frame.width(),
        "column count differs between wires"
    );

    // Arrow carries the source f64 bits verbatim, so the Arrow-decoded frame is
    // ground truth. Compare the JSON-decoded frame against it per numeric column
    // (bit-for-bit). Passthrough columns from the CSV survive serde_json's fast
    // parser; the computed `score` column does not.
    let numeric: Vec<&str> = defs
        .iter()
        .filter(|d| d.dtype != "string")
        .map(|d| d.name.as_str())
        .collect();
    let drift: Vec<(&str, usize)> = numeric
        .iter()
        .map(|&c| (c, col_bit_drift(&json_frame, &arrow_frame, c)))
        .collect();
    let total_drift: usize = drift.iter().map(|(_, c)| c).sum();

    println!(
        "value fidelity vs the bit-exact Arrow wire ({} rows):",
        json_frame.height()
    );
    println!("  arrow   exact by construction — carries the source f64 bits");
    if total_drift == 0 {
        println!("  json    exact ✓\n");
    } else {
        println!("  json    {total_drift} float cell(s) corrupted by ≤1 ULP (serde_json's default parser):");
        for (name, c) in drift.iter().filter(|(_, c)| *c > 0) {
            println!("            {name:<7} {c}/{} rows", json_frame.height());
        }
        println!();
    }

    // --- Timings ------------------------------------------------------------
    // "call" times the export: in-guest (wasm) encode plus the copy of the
    // result out of guest linear memory across the component boundary.
    // "decode" times the host turning those bytes into a polars frame.
    let json_call_us = time_avg_us(iters, || {
        let _ = frames.call_rows_json(&mut store, n).unwrap();
    });
    let arrow_call_us = time_avg_us(iters, || {
        let _ = frames.call_rows_arrow(&mut store, n).unwrap();
    });
    let json_decode_us = time_avg_us(iters, || {
        let v: Value = serde_json::from_str(&json_wire).unwrap();
        let _ = DataFrame::new(defs.clone(), v, None).to_dataframe().unwrap();
    });
    let arrow_decode_us = time_avg_us(iters, || {
        let _ = from_arrow_ipc(&arrow_wire).unwrap();
    });

    let size_ratio = json_bytes as f64 / arrow_bytes as f64;
    let call_ratio = json_call_us / arrow_call_us;
    let decode_ratio = json_decode_us / arrow_decode_us;
    let json_total = json_call_us + json_decode_us;
    let arrow_total = arrow_call_us + arrow_decode_us;
    let total_ratio = json_total / arrow_total;

    let speed = |r: f64| {
        if r >= 1.0 {
            format!("{r:.1}× faster")
        } else {
            format!("{:.1}× slower", 1.0 / r)
        }
    };
    // Rows: `wire size` = bytes on the wire; `encode + transfer` = in-wasm
    // encode + the copy of the result out of guest memory; `decode (host)` =
    // host turning those bytes into a polars table; `total` = the per-call cost
    // a host pays end to end. (Spelled out for readers in the README.)
    println!("  {:<18}{:>14}{:>14}    advantage", "", "JSON", "Arrow");
    println!(
        "  {:<18}{:>14}{:>14}    {:.2}× smaller",
        "wire size",
        format!("{json_bytes} B"),
        format!("{arrow_bytes} B"),
        size_ratio
    );
    println!(
        "  {:<18}{:>14}{:>14}    {}",
        "encode + transfer",
        format!("{json_call_us:.0} µs"),
        format!("{arrow_call_us:.0} µs"),
        speed(call_ratio)
    );
    println!(
        "  {:<18}{:>14}{:>14}    {}",
        "decode (host)",
        format!("{json_decode_us:.0} µs"),
        format!("{arrow_decode_us:.0} µs"),
        speed(decode_ratio)
    );
    println!(
        "  {:<18}{:>14}{:>14}    {}",
        "total",
        format!("{json_total:.0} µs"),
        format!("{arrow_total:.0} µs"),
        speed(total_ratio)
    );

    Ok(())
}
