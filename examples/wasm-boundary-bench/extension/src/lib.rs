//! databench — a "large dataset" variation of the kv example extension.
//!
//! It bakes a sample of the `diamonds` dataset (53k-row classic; we ship a 5k
//! sample and cycle it) and serves the first `n` rows two ways across the real
//! component boundary: as a JSON array-of-objects string (today's
//! `data-frame-output.rows` wire) and as an Arrow IPC file buffer (the R1 wire,
//! encoded in-guest with arrow-rs — the R1c path). The host harness times both.

use std::sync::{Arc, OnceLock};

use arrow_array::{Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::FileWriter;
use arrow_schema::{DataType, Field, Schema};
use serde_json::{Value, json};

wit_bindgen::generate!({
    world: "databench",
    path: "wit",
});

use exports::bench::databench::frames::{Column, Guest};

struct Component;
export!(Component);

/// The baked fixture — a 5,000-row sample of the seaborn `diamonds` mirror.
const CSV: &str = include_str!("../diamonds.csv");

/// Struct-of-arrays form of the dataset, parsed once.
#[derive(Default)]
struct Dataset {
    carat: Vec<f64>,
    cut: Vec<String>,
    color: Vec<String>,
    clarity: Vec<String>,
    depth: Vec<f64>,
    table: Vec<f64>,
    price: Vec<i64>,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
}

/// Parse-once cache. wasm components are single-threaded, so `OnceLock` gives a
/// cheap `&'static Dataset` with no contention. Parsing is excluded from every
/// per-call measurement this way — calls only pay for encoding.
fn data() -> &'static Dataset {
    static DATA: OnceLock<Dataset> = OnceLock::new();
    DATA.get_or_init(parse_csv)
}

fn unquote(s: &str) -> String {
    s.trim_matches('"').to_string()
}

fn parse_csv() -> Dataset {
    let mut d = Dataset::default();
    for line in CSV.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 10 {
            continue;
        }
        d.carat.push(f[0].parse().unwrap_or(0.0));
        d.cut.push(unquote(f[1]));
        d.color.push(unquote(f[2]));
        d.clarity.push(unquote(f[3]));
        d.depth.push(f[4].parse().unwrap_or(0.0));
        d.table.push(f[5].parse().unwrap_or(0.0));
        d.price.push(f[6].parse().unwrap_or(0));
        d.x.push(f[7].parse().unwrap_or(0.0));
        d.y.push(f[8].parse().unwrap_or(0.0));
        d.z.push(f[9].parse().unwrap_or(0.0));
    }
    d
}

/// Indices into the fixture for the first `n` logical rows, cycling the sample
/// when `n` exceeds the fixture size.
fn cycled_indices(n: u32, total: usize) -> Vec<usize> {
    let n = n as usize;
    (0..n).map(|i| i % total).collect()
}

/// A *computed* column (a derived metric an extension might return). The
/// arithmetic produces full-precision f64s whose shortest decimal string a
/// fast JSON parser rounds to ±1 ULP — exactly the values that expose the
/// JSON wire's float corruption. The raw f64 carried by Arrow does not drift.
fn score_at(d: &Dataset, j: usize) -> f64 {
    d.price[j] as f64 * 0.37 + d.carat[j] * 100.0
}

impl Guest for Component {
    fn schema() -> Vec<Column> {
        let col = |name: &str, dtype: &str| Column {
            name: name.to_string(),
            dtype: dtype.to_string(),
        };
        vec![
            col("carat", "number"),
            col("cut", "string"),
            col("color", "string"),
            col("clarity", "string"),
            col("depth", "number"),
            col("table", "number"),
            col("price", "integer"),
            col("x", "number"),
            col("y", "number"),
            col("z", "number"),
            col("score", "number"), // computed: price*0.37 + carat*100
        ]
    }

    fn total_rows() -> u32 {
        data().carat.len() as u32
    }

    fn rows_json(n: u32) -> String {
        let d = data();
        let total = d.carat.len();
        let mut arr = Vec::with_capacity(n as usize);
        for j in cycled_indices(n, total) {
            arr.push(json!({
                "carat": d.carat[j],
                "cut": d.cut[j],
                "color": d.color[j],
                "clarity": d.clarity[j],
                "depth": d.depth[j],
                "table": d.table[j],
                "price": d.price[j],
                "x": d.x[j],
                "y": d.y[j],
                "z": d.z[j],
                "score": score_at(d, j),
            }));
        }
        serde_json::to_string(&Value::Array(arr)).unwrap_or_default()
    }

    fn rows_arrow(n: u32) -> Vec<u8> {
        let d = data();
        let total = d.carat.len();
        let idx = cycled_indices(n, total);

        let carat = Float64Array::from_iter_values(idx.iter().map(|&j| d.carat[j]));
        let cut = StringArray::from_iter_values(idx.iter().map(|&j| d.cut[j].as_str()));
        let color = StringArray::from_iter_values(idx.iter().map(|&j| d.color[j].as_str()));
        let clarity = StringArray::from_iter_values(idx.iter().map(|&j| d.clarity[j].as_str()));
        let depth = Float64Array::from_iter_values(idx.iter().map(|&j| d.depth[j]));
        let table = Float64Array::from_iter_values(idx.iter().map(|&j| d.table[j]));
        let price = Int64Array::from_iter_values(idx.iter().map(|&j| d.price[j]));
        let x = Float64Array::from_iter_values(idx.iter().map(|&j| d.x[j]));
        let y = Float64Array::from_iter_values(idx.iter().map(|&j| d.y[j]));
        let z = Float64Array::from_iter_values(idx.iter().map(|&j| d.z[j]));
        let score = Float64Array::from_iter_values(idx.iter().map(|&j| score_at(d, j)));

        let schema = Arc::new(Schema::new(vec![
            Field::new("carat", DataType::Float64, false),
            Field::new("cut", DataType::Utf8, false),
            Field::new("color", DataType::Utf8, false),
            Field::new("clarity", DataType::Utf8, false),
            Field::new("depth", DataType::Float64, false),
            Field::new("table", DataType::Float64, false),
            Field::new("price", DataType::Int64, false),
            Field::new("x", DataType::Float64, false),
            Field::new("y", DataType::Float64, false),
            Field::new("z", DataType::Float64, false),
            Field::new("score", DataType::Float64, false),
        ]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(carat),
                Arc::new(cut),
                Arc::new(color),
                Arc::new(clarity),
                Arc::new(depth),
                Arc::new(table),
                Arc::new(price),
                Arc::new(x),
                Arc::new(y),
                Arc::new(z),
                Arc::new(score),
            ],
        )
        .expect("record batch");

        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = FileWriter::try_new(&mut buf, &schema).expect("ipc file writer");
            w.write(&batch).expect("write batch");
            w.finish().expect("finish ipc");
        }
        buf
    }
}
