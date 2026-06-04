//! DataFrame tool output type.

use super::Label;
use crate::{Error, column};
use polars_core::prelude::*;
use polars_io::ipc::{IpcReader, IpcWriter};
use polars_io::{SerReader, SerWriter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Columnar data that can be converted to a polars DataFrame on the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFrame {
    /// Schema describing each column.
    pub schema: Vec<column::Def>,
    /// Columnar data as an Arrow IPC byte buffer — the wire form that replaced
    /// the legacy JSON `rows` string. Decode with [`Self::to_dataframe`]. Empty
    /// when the frame carries no rows.
    pub frame_ipc: Vec<u8>,
    /// Optional free-form metadata attached to the frame.
    pub metadata: Option<Value>,
    /// Optional label for display (e.g., "AAPL" for stock data).
    pub label: Option<Label>,
    /// Human-readable description of the invocation (e.g., the SQL query).
    #[serde(default)]
    pub source: Option<String>,
    /// Per-output cache override — when set, overrides `tool::Info.cacheable`.
    #[serde(default)]
    pub store: Option<bool>,
    /// Brief description of what the output contains.
    #[serde(default)]
    pub description: Option<String>,
    /// Short, no-spaces identifier for use in formulas (e.g., "combined_ratio").
    #[serde(default)]
    pub nickname: Option<String>,
}

impl DataFrame {
    /// Create a DataFrame output from an Arrow IPC buffer — the canonical wire
    /// form. Infallible: the buffer is stored as-is and decoded lazily by
    /// [`Self::to_dataframe`].
    pub fn new(schema: Vec<column::Def>, frame_ipc: Vec<u8>, metadata: Option<Value>) -> Self {
        Self {
            schema,
            frame_ipc,
            metadata,
            label: None,
            source: None,
            store: None,
            description: None,
            nickname: None,
        }
    }

    /// Create a DataFrame output from an Arrow IPC buffer with a label.
    pub fn with_label(schema: Vec<column::Def>, frame_ipc: Vec<u8>, metadata: Option<Value>, label: Label) -> Self {
        Self {
            schema,
            frame_ipc,
            metadata,
            label: Some(label),
            source: None,
            store: None,
            description: None,
            nickname: None,
        }
    }

    /// Build a DataFrame output from JSON data by encoding it to Arrow IPC.
    /// Convenience for host-side/test code that still holds a JSON [`Value`];
    /// the extension→host wire never takes this path (the guest writes Arrow
    /// directly). `Value::Null` yields an empty frame.
    pub fn from_json(schema: Vec<column::Def>, data: Value, metadata: Option<Value>) -> Result<Self, Error> {
        let frame_ipc = if data.is_null() {
            Vec::new()
        } else {
            to_arrow_ipc(&schema, &data)?
        };
        Ok(Self::new(schema, frame_ipc, metadata))
    }

    /// Decode the Arrow IPC buffer into a polars DataFrame — the host accessor.
    /// Signature is stable across R1 (host call sites untouched); only the
    /// internals moved from a JSON parse to an Arrow read. Empty buffer → empty
    /// frame.
    pub fn to_dataframe(self) -> Result<polars_core::frame::DataFrame, Error> {
        if self.frame_ipc.is_empty() {
            return Ok(polars_core::frame::DataFrame::empty());
        }
        from_arrow_ipc(&self.frame_ipc)
    }

    /// Decode the frame and re-serialize as row-oriented JSON objects, keyed by
    /// column (display) name. Transitional bridge for consumers still on JSON —
    /// notably the JSON-backed host-data registry (Arrow-ified in R2) and demo
    /// rendering. The hot extension→host path never takes this; it stays
    /// columnar end to end.
    pub fn to_json_rows(&self) -> Result<Vec<Value>, Error> {
        if self.frame_ipc.is_empty() {
            return Ok(Vec::new());
        }
        let frame = from_arrow_ipc(&self.frame_ipc)?;
        let cols = frame.get_columns();
        let mut rows = Vec::with_capacity(frame.height());
        for i in 0..frame.height() {
            let mut obj = serde_json::Map::with_capacity(cols.len());
            for col in cols {
                let val = col
                    .get(i)
                    .map_err(|e| Error::DataFrame(format!("row {i} col '{}': {e}", col.name())))?;
                obj.insert(col.name().to_string(), anyvalue_to_json(val));
            }
            rows.push(Value::Object(obj));
        }
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// JSON → polars (the legacy decode path; now the Arrow encode-side feeder and
// a public benchmark/host convenience)
// ---------------------------------------------------------------------------

/// Build a polars DataFrame from a column schema + JSON data value. Accepts
/// row-oriented (`[{col: val, …}, …]`) or column-oriented (`{col: [val, …]}`)
/// shapes. Feeds [`to_arrow_ipc`] and backs [`DataFrame::from_json`]; exposed
/// so the wasm-boundary bench can time the legacy JSON→polars path directly
/// against the Arrow path.
pub fn json_to_polars(schema: &[column::Def], data: &Value) -> Result<polars_core::frame::DataFrame, Error> {
    // Helper to convert string dtype to Polars DataType
    fn to_dtype(dtype: &str) -> DataType {
        match dtype {
            "string" => DataType::String,
            "number" | "float" => DataType::Float64,
            "integer" | "int" => DataType::Int64,
            "boolean" | "bool" => DataType::Boolean,
            "date" => DataType::Date,
            "datetime" => DataType::Datetime(TimeUnit::Milliseconds, None),
            _ => DataType::String, // Default to string for unknown types
        }
    }

    // Helper to parse a value as f64, handling both number and string representations
    fn parse_as_f64(value: &serde_json::Value) -> Option<f64> {
        match value {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    // Helper to parse a value as i64, handling both number and string representations
    fn parse_as_i64(value: &serde_json::Value) -> Option<i64> {
        match value {
            serde_json::Value::Number(n) => n.as_i64(),
            serde_json::Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    // Helper to parse a value as bool, handling various representations
    fn parse_as_bool(value: &serde_json::Value) -> Option<bool> {
        match value {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => None,
            },
            serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0),
            _ => None,
        }
    }

    match data {
        Value::Array(arr) => {
            if arr.is_empty() {
                return Ok(polars_core::frame::DataFrame::empty());
            }

            // Create Series for each column based on schema
            let mut columns_vec = Vec::new();

            for col_def in schema {
                let dtype = to_dtype(&col_def.dtype);
                let alias = col_def.alias.clone().into();

                let series = match dtype {
                    DataType::String | DataType::Date => {
                        let values: Vec<Option<String>> = arr
                            .iter()
                            .map(|item| {
                                item.get(&col_def.name).and_then(|v| match v {
                                    serde_json::Value::String(s) => Some(s.clone()),
                                    serde_json::Value::Null => None,
                                    other => Some(other.to_string()),
                                })
                            })
                            .collect();
                        Series::new(alias, values)
                    }
                    DataType::Float64 => {
                        let values: Vec<Option<f64>> = arr
                            .iter()
                            .map(|item| item.get(&col_def.name).and_then(parse_as_f64))
                            .collect();
                        Series::new(alias, values)
                    }
                    DataType::Int64 => {
                        let values: Vec<Option<i64>> = arr
                            .iter()
                            .map(|item| item.get(&col_def.name).and_then(parse_as_i64))
                            .collect();
                        Series::new(alias, values)
                    }
                    DataType::Boolean => {
                        let values: Vec<Option<bool>> = arr
                            .iter()
                            .map(|item| item.get(&col_def.name).and_then(parse_as_bool))
                            .collect();
                        Series::new(alias, values)
                    }
                    _ => {
                        // Default to string representation for other types
                        let values: Vec<Option<String>> = arr
                            .iter()
                            .map(|item| {
                                item.get(&col_def.name).and_then(|v| match v {
                                    serde_json::Value::Null => None,
                                    other => Some(other.to_string()),
                                })
                            })
                            .collect();
                        Series::new(alias, values)
                    }
                };
                columns_vec.push(Column::Series(series.into()));
            }

            polars_core::frame::DataFrame::new(columns_vec)
                .map_err(|e| Error::DataFrame(format!("Failed to create DataFrame: {}", e)))
        }

        Value::Object(obj) => {
            // For object format, assume it's column-oriented data
            // e.g., {"col1": [1,2,3], "col2": ["a","b","c"]}
            let mut columns_vec = Vec::new();

            // Track the maximum number of rows across all columns
            let mut max_rows = 0usize;

            for col_def in schema {
                let alias = col_def.alias.clone().into();
                let dtype = to_dtype(&col_def.dtype);

                if let Some(col_data) = obj.get(&col_def.name) {
                    let series = match col_data {
                        Value::Array(values) => {
                            max_rows = max_rows.max(values.len());

                            match dtype {
                                DataType::String | DataType::Date => {
                                    let parsed: Vec<Option<String>> = values
                                        .iter()
                                        .map(|v| match v {
                                            Value::Null => None,
                                            Value::String(s) => Some(s.clone()),
                                            other => Some(other.to_string()),
                                        })
                                        .collect();
                                    Series::new(alias, parsed)
                                }
                                DataType::Float64 => {
                                    let parsed: Vec<Option<f64>> = values.iter().map(parse_as_f64).collect();
                                    Series::new(alias, parsed)
                                }
                                DataType::Int64 => {
                                    let parsed: Vec<Option<i64>> = values.iter().map(parse_as_i64).collect();
                                    Series::new(alias, parsed)
                                }
                                DataType::Boolean => {
                                    let parsed: Vec<Option<bool>> = values.iter().map(parse_as_bool).collect();
                                    Series::new(alias, parsed)
                                }
                                _ => {
                                    // Default to string
                                    let parsed: Vec<Option<String>> = values
                                        .iter()
                                        .map(|v| match v {
                                            Value::Null => None,
                                            Value::String(s) => Some(s.clone()),
                                            other => Some(other.to_string()),
                                        })
                                        .collect();
                                    Series::new(alias, parsed)
                                }
                            }
                        }
                        // If it's not an array, treat it as a single value repeated for all rows
                        // This will be adjusted after we know the max_rows
                        _ => {
                            // For now, create a single-element series
                            match dtype {
                                DataType::String | DataType::Date => {
                                    let value = match col_data {
                                        Value::Null => None,
                                        Value::String(s) => Some(s.clone()),
                                        other => Some(other.to_string()),
                                    };
                                    Series::new(alias, vec![value])
                                }
                                DataType::Float64 => Series::new(alias, vec![parse_as_f64(col_data)]),
                                DataType::Int64 => Series::new(alias, vec![parse_as_i64(col_data)]),
                                DataType::Boolean => Series::new(alias, vec![parse_as_bool(col_data)]),
                                _ => {
                                    let value = match col_data {
                                        Value::Null => None,
                                        Value::String(s) => Some(s.clone()),
                                        other => Some(other.to_string()),
                                    };
                                    Series::new(alias, vec![value])
                                }
                            }
                        }
                    };
                    columns_vec.push(Column::Series(series.into()));
                } else {
                    // Column not found in data, create empty column with nulls
                    // We'll resize it to max_rows after processing all columns
                    let series = match dtype {
                        DataType::String | DataType::Date => Series::new(alias, Vec::<Option<String>>::new()),
                        DataType::Float64 => Series::new(alias, Vec::<Option<f64>>::new()),
                        DataType::Int64 => Series::new(alias, Vec::<Option<i64>>::new()),
                        DataType::Boolean => Series::new(alias, Vec::<Option<bool>>::new()),
                        _ => Series::new(alias, Vec::<Option<String>>::new()),
                    };
                    columns_vec.push(Column::Series(series.into()));
                }
            }

            // If we have columns, ensure they all have the same length
            // This handles cases where some columns might have been single values or missing
            if !columns_vec.is_empty() && max_rows > 0 {
                for col in columns_vec.iter_mut() {
                    if let Column::Series(series) = col {
                        let current_len = series.len();
                        if current_len == 0 {
                            // Create a series of nulls with the right length
                            let dtype = series.dtype();
                            let name = series.name().clone();
                            let new_series = match dtype {
                                DataType::String => Series::new(name, vec![Option::<String>::None; max_rows]),
                                DataType::Float64 => Series::new(name, vec![Option::<f64>::None; max_rows]),
                                DataType::Int64 => Series::new(name, vec![Option::<i64>::None; max_rows]),
                                DataType::Boolean => Series::new(name, vec![Option::<bool>::None; max_rows]),
                                _ => Series::new(name, vec![Option::<String>::None; max_rows]),
                            };
                            *series = new_series.into();
                        } else if current_len == 1 && max_rows > 1 {
                            // Repeat the single value for all rows
                            // This is a broadcast operation
                            let extended = series.new_from_index(0, max_rows);
                            *series = extended.into();
                        }
                        // If current_len matches max_rows, nothing to do
                    }
                }
            }

            polars_core::frame::DataFrame::new(columns_vec)
                .map_err(|e| Error::DataFrame(format!("Failed to create DataFrame: {}", e)))
        }

        Value::Null => Ok(polars_core::frame::DataFrame::empty()),

        _ => Err(Error::DataFrame(
            "Data must be an array or object to convert to DataFrame".to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Arrow IPC codec — the data-frame wire format (R1, see
// docs/extension-data-transport-roadmap.md and d:extension-payload-strategy)
// ---------------------------------------------------------------------------

/// Encode `(schema, data)` as an Arrow IPC byte buffer — the columnar wire
/// format that replaces the legacy JSON `rows` string on the extension→host
/// boundary. The host decodes it with [`from_arrow_ipc`] with no JSON parse and
/// no per-cell allocation.
///
/// In R1 the encode side stands in for the extension/guest (which will use an
/// `arrow-rs` writer in R1c). The IPC wire format is identical regardless of
/// which library wrote it, so a polars-written buffer here proves the same
/// decode contract the guest will exercise.
pub fn to_arrow_ipc(schema: &[column::Def], data: &Value) -> Result<Vec<u8>, Error> {
    let mut df = json_to_polars(schema, data)?;
    let mut buf = Vec::new();
    IpcWriter::new(&mut buf)
        .finish(&mut df)
        .map_err(|e| Error::DataFrame(format!("Arrow IPC write failed: {e}")))?;
    Ok(buf)
}

/// Decode an Arrow IPC byte buffer into a polars DataFrame — the host side of
/// the data-frame wire format. This is the call that replaces the JSON parse +
/// per-cell box of the legacy path.
pub fn from_arrow_ipc(bytes: &[u8]) -> Result<polars_core::frame::DataFrame, Error> {
    IpcReader::new(std::io::Cursor::new(bytes))
        .finish()
        .map_err(|e| Error::DataFrame(format!("Arrow IPC read failed: {e}")))
}

/// A decoded, sliceable Arrow frame for the host-data registry (R2). Wraps a
/// polars `DataFrame` so the host crate can window host-managed data zero-copy
/// **without naming polars itself** — keeping the polars dependency contained in
/// `emporium-core`, the same boundary the guest enforces.
///
/// Lifecycle: decode once on `register` ([`Self::from_ipc`]), then slice per
/// cursor `next-batch` ([`Self::batch_ipc`]). The slice is a zero-copy view that
/// shares the underlying Arrow buffers; only the emitted window is copied, and
/// only when it is re-encoded to an IPC batch for the WIT boundary.
#[derive(Clone)]
pub struct StoredFrame {
    frame: polars_core::frame::DataFrame,
}

impl StoredFrame {
    /// Decode an Arrow IPC buffer into a stored frame — one decode per
    /// `register`, amortized across every subsequent window. An empty buffer
    /// yields an empty frame (mirroring [`DataFrame::to_dataframe`]), so callers
    /// can register a placeholder resource before any rows exist.
    pub fn from_ipc(bytes: &[u8]) -> Result<Self, Error> {
        let frame = if bytes.is_empty() {
            polars_core::frame::DataFrame::empty()
        } else {
            from_arrow_ipc(bytes)?
        };
        Ok(Self { frame })
    }

    /// Total number of rows held.
    pub fn row_count(&self) -> usize {
        self.frame.height()
    }

    /// Encode the window `[offset, offset + len)` as a standalone Arrow IPC
    /// batch. Returns `None` at or past EOF (and for a zero-length request,
    /// mirroring the legacy empty-slice EOF signal); otherwise the IPC bytes plus
    /// the number of rows actually emitted (which may be `< len` at the tail) so
    /// the caller can advance its cursor offset. The `slice` is a zero-copy view;
    /// only the window pays a copy, when written to IPC.
    pub fn batch_ipc(&self, offset: usize, len: usize) -> Result<Option<(Vec<u8>, usize)>, Error> {
        let height = self.frame.height();
        if offset >= height || len == 0 {
            return Ok(None);
        }
        let take = len.min(height - offset);
        let mut window = self.frame.slice(offset as i64, take);
        let mut buf = Vec::new();
        IpcWriter::new(&mut buf)
            .finish(&mut window)
            .map_err(|e| Error::DataFrame(format!("Arrow IPC batch write failed: {e}")))?;
        Ok(Some((buf, take)))
    }
}

/// Convert a single polars `AnyValue` to a JSON value. Used by
/// [`DataFrame::to_json_rows`]; covers the dtypes the JSON wire ever produced
/// (text / number / bool / null), with a stringified fallback for the rest.
fn anyvalue_to_json(v: AnyValue) -> Value {
    match v {
        AnyValue::Null => Value::Null,
        AnyValue::Boolean(b) => Value::Bool(b),
        AnyValue::String(s) => Value::String(s.to_string()),
        AnyValue::StringOwned(s) => Value::String(s.to_string()),
        AnyValue::Int8(n) => Value::from(n),
        AnyValue::Int16(n) => Value::from(n),
        AnyValue::Int32(n) => Value::from(n),
        AnyValue::Int64(n) => Value::from(n),
        AnyValue::UInt8(n) => Value::from(n),
        AnyValue::UInt16(n) => Value::from(n),
        AnyValue::UInt32(n) => Value::from(n),
        AnyValue::UInt64(n) => Value::from(n),
        AnyValue::Float32(f) => serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        AnyValue::Float64(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        other => Value::String(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn two_col_schema() -> Vec<column::Def> {
        vec![
            column::Def {
                name: "a".to_string(),
                alias: "A".to_string(),
                dtype: "integer".to_string(),
            },
            column::Def {
                name: "b".to_string(),
                alias: "B".to_string(),
                dtype: "string".to_string(),
            },
        ]
    }

    #[test]
    fn data_frame_from_json_populates_schema_and_metadata() {
        let schema = two_col_schema();
        let data = json!([{"a": 1, "b": "x"}]);
        let metadata = Some(json!({"src": "test"}));
        let df = DataFrame::from_json(schema.clone(), data, metadata.clone()).unwrap();
        assert_eq!(df.schema.len(), 2);
        assert!(
            !df.frame_ipc.is_empty(),
            "json data should encode to a non-empty Arrow buffer"
        );
        assert_eq!(df.metadata, metadata);
        assert!(df.label.is_none());
        assert!(df.source.is_none());
        assert!(df.store.is_none());
        assert!(df.description.is_none());
        assert!(df.nickname.is_none());
        assert_eq!(df.to_dataframe().unwrap().height(), 1);
    }

    #[test]
    fn data_frame_with_label_sets_label() {
        let schema = two_col_schema();
        let df = DataFrame::with_label(schema, Vec::new(), None, Label::new("L"));
        assert_eq!(df.label.as_ref().map(|l| l.0.as_str()), Some("L"));
    }

    #[test]
    fn json_to_polars_row_oriented() {
        let schema = two_col_schema();
        let data = json!([
            {"a": 1, "b": "x"},
            {"a": 2, "b": "y"},
        ]);
        let polars_df = json_to_polars(&schema, &data).unwrap();
        assert_eq!(polars_df.height(), 2);
        assert_eq!(polars_df.width(), 2);
    }

    #[test]
    fn json_to_polars_empty_array_is_empty_frame() {
        let schema = two_col_schema();
        let polars_df = json_to_polars(&schema, &json!([])).unwrap();
        assert_eq!(polars_df.height(), 0);
    }

    #[test]
    fn to_dataframe_from_empty_buffer_is_empty_frame() {
        let schema = two_col_schema();
        // Null JSON encodes to an empty buffer; to_dataframe must yield an empty
        // frame rather than failing on a zero-length IPC read.
        let df = DataFrame::from_json(schema, serde_json::Value::Null, None).unwrap();
        assert!(df.frame_ipc.is_empty());
        assert_eq!(df.to_dataframe().unwrap().height(), 0);
    }

    #[test]
    fn from_json_invalid_data_returns_error() {
        let schema = two_col_schema();
        let err = DataFrame::from_json(schema.clone(), json!("just a string"), None).unwrap_err();
        assert!(matches!(err, Error::DataFrame(_)));

        let err = DataFrame::from_json(schema, json!(42), None).unwrap_err();
        assert!(matches!(err, Error::DataFrame(_)));
    }

    #[test]
    fn json_to_polars_column_oriented_object_format() {
        let schema = two_col_schema();
        let data = json!({
            "a": [1, 2, 3],
            "b": ["x", "y", "z"],
        });
        let polars_df = json_to_polars(&schema, &data).unwrap();
        assert_eq!(polars_df.height(), 3);
        assert_eq!(polars_df.width(), 2);
    }

    // -- Arrow IPC round-trip (R1) ------------------------------------------

    #[test]
    fn arrow_ipc_round_trips_row_oriented() {
        let schema = two_col_schema();
        let data = json!([
            {"a": 1, "b": "x"},
            {"a": 2, "b": "y"},
            {"a": 3, "b": "z"},
        ]);
        // The host decode (from_arrow_ipc ∘ to_arrow_ipc) must equal the legacy
        // direct build — same frame, byte-for-byte equivalent contents.
        let direct = json_to_polars(&schema, &data).unwrap();
        let ipc = to_arrow_ipc(&schema, &data).unwrap();
        let decoded = from_arrow_ipc(&ipc).unwrap();
        assert_eq!(decoded.height(), 3);
        assert_eq!(decoded.width(), 2);
        assert!(decoded.equals(&direct), "IPC round-trip diverged from direct build");
    }

    #[test]
    fn arrow_ipc_round_trips_column_oriented() {
        let schema = two_col_schema();
        let data = json!({
            "a": [1, 2, 3, 4],
            "b": ["w", "x", "y", "z"],
        });
        let direct = json_to_polars(&schema, &data).unwrap();
        let ipc = to_arrow_ipc(&schema, &data).unwrap();
        let decoded = from_arrow_ipc(&ipc).unwrap();
        assert_eq!(decoded.height(), 4);
        assert!(decoded.equals(&direct), "IPC round-trip diverged from direct build");
    }

    #[test]
    fn arrow_ipc_preserves_dtypes_and_nulls() {
        let schema = vec![
            column::Def {
                name: "n".into(),
                alias: "N".into(),
                dtype: "number".into(),
            },
            column::Def {
                name: "flag".into(),
                alias: "Flag".into(),
                dtype: "boolean".into(),
            },
        ];
        let data = json!([
            {"n": 1.5, "flag": true},
            {"n": null, "flag": false},
            {"n": 2.25, "flag": null},
        ]);
        let direct = json_to_polars(&schema, &data).unwrap();
        let decoded = from_arrow_ipc(&to_arrow_ipc(&schema, &data).unwrap()).unwrap();
        // Dtypes survive the IPC round-trip (Float64 + Boolean, not stringified).
        assert_eq!(decoded.dtypes(), direct.dtypes());
        assert!(decoded.equals_missing(&direct), "nulls or values diverged across IPC");
    }

    #[test]
    fn arrow_ipc_preserves_inexact_arithmetic_floats_bit_for_bit() {
        // Values like `base + 0.4` are not exactly representable; the bits an
        // extension computes can differ from the bits you get by parsing their
        // shortest decimal string (which is what a JSON wire round-trips
        // through). Arrow carries the source f64 verbatim, so the IPC round-trip
        // is bit-exact — the property the data-transport design relies on.
        let schema = vec![column::Def {
            name: "v".into(),
            alias: "v".into(),
            dtype: "number".into(),
        }];
        let vals: Vec<f64> = (0..256).map(|i| 100.0 + i as f64 * 0.37 + 0.4).collect();
        let data = Value::Array(vals.iter().map(|v| json!({ "v": v })).collect());

        let decoded = from_arrow_ipc(&to_arrow_ipc(&schema, &data).unwrap()).unwrap();
        let col = decoded.column("v").unwrap().as_materialized_series().f64().unwrap();
        for (i, want) in vals.iter().enumerate() {
            assert_eq!(
                col.get(i).unwrap().to_bits(),
                want.to_bits(),
                "row {i} drifted across IPC"
            );
        }
    }

    #[test]
    fn arrow_ipc_error_on_garbage_bytes() {
        let err = from_arrow_ipc(b"not an arrow ipc stream").unwrap_err();
        assert!(matches!(err, Error::DataFrame(_)));
    }
}
