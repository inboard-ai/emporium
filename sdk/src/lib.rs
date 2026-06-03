//! Emporium SDK — the guest-side toolkit for writing extensions.
//!
//! The extension↔host data wire is an **Arrow IPC** byte buffer (see
//! `docs/extension-data-transport-roadmap.md` and `d:extension-payload-strategy`).
//! This crate wraps `arrow-rs` so extension authors build columnar output through
//! a small typed [`Frame`] builder and **never depend on `arrow-rs` directly** —
//! shielding every extension (and the marketplace's third-party authors) from
//! arrow's frequent breaking releases.
//!
//! The contract with the host is the WIT/ABI, not this crate: any extension, in
//! any language, that emits a valid Arrow IPC *file* buffer is equally valid.
//! [`Frame`] is the Rust convenience, and [`Frame::from_ipc_bytes`] is the escape
//! hatch for authors who build the bytes another way.
//!
//! ```no_run
//! use emporium_sdk::Frame;
//! let frame = Frame::builder()
//!     .text("symbol", "Symbol", ["AAPL".to_string(), "MSFT".to_string()])
//!     .number("close", "Close", [195.0_f64, 410.5]) // native f64 — bit-exact, no JSON
//!     .build()
//!     .expect("rectangular columns");
//! let (schema, arrow_ipc) = frame.into_parts();
//! // Drop `arrow_ipc` into your generated `DataFrameOutput.arrow_ipc`, and map
//! // `schema` into its `schema: Vec<ColumnDef>` (a 3-field copy).
//! ```

use std::sync::Arc;

use arrow_array::{Array, ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::FileWriter;
use arrow_schema::{DataType, Field, Schema};

/// A column's schema descriptor: `name` (canonical id), `alias` (display name,
/// also the Arrow field name), and a `dtype` tag matching the host's
/// `source::Kind` vocabulary (`string | number | integer | boolean | date`).
///
/// Deliberately a plain struct, not the host's `column::Def` (which lives in the
/// polars-bearing `emporium-core`): the SDK stays guest-lean, and authors map
/// these three fields into whatever `ColumnDef` their `wit_bindgen::generate!`
/// produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    /// Canonical column id.
    pub name: String,
    /// Display name; used verbatim as the Arrow field name in the IPC buffer.
    pub alias: String,
    /// Host dtype tag: `string | number | integer | boolean | date`.
    pub dtype: String,
}

/// Errors from building a [`Frame`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Columns must be rectangular: every column must have the same row count.
    #[error("column '{column}' has {got} rows but the frame has {expected}")]
    RaggedColumns {
        /// The offending column's alias.
        column: String,
        /// Row count established by the first column.
        expected: usize,
        /// This column's row count.
        got: usize,
    },
    /// The underlying arrow encode failed.
    #[error("arrow encode failed: {0}")]
    Arrow(String),
}

/// A built data frame: the column schema plus the Arrow IPC byte buffer that
/// crosses the WIT boundary as `data-frame-output.arrow-ipc`.
#[derive(Debug, Clone)]
pub struct Frame {
    schema: Vec<ColumnDef>,
    ipc: Vec<u8>,
}

impl Frame {
    /// Start building a frame column by column.
    pub fn builder() -> FrameBuilder {
        FrameBuilder { cols: Vec::new() }
    }

    /// The column schema descriptors, in column order.
    pub fn schema(&self) -> &[ColumnDef] {
        &self.schema
    }

    /// The Arrow IPC file bytes — the value for `data-frame-output.arrow-ipc`.
    pub fn arrow_ipc(&self) -> &[u8] {
        &self.ipc
    }

    /// Consume into `(schema, arrow_ipc)` for assembling the WIT output.
    pub fn into_parts(self) -> (Vec<ColumnDef>, Vec<u8>) {
        (self.schema, self.ipc)
    }

    /// Escape hatch: wrap an Arrow IPC buffer produced by any means (another
    /// arrow library, another language) together with its schema description.
    /// The bytes must be a valid Arrow IPC *file* stream — the host reads them
    /// with polars.
    pub fn from_ipc_bytes(schema: Vec<ColumnDef>, arrow_ipc: Vec<u8>) -> Self {
        Frame { schema, ipc: arrow_ipc }
    }
}

struct BuiltColumn {
    def: ColumnDef,
    field: Field,
    array: ArrayRef,
}

/// Accumulates typed columns, then encodes them to one Arrow IPC buffer.
///
/// Column values are **native Rust types** (`f64`, `i64`, `String`, `bool`), not
/// JSON — so a computed float reaches the host with its exact bits, never routed
/// through a decimal string. Each method accepts an iterator of values or
/// `Option`s (a bare value is a non-null cell): `[1.0, 2.0]` and
/// `[Some(1.0), None]` both work.
pub struct FrameBuilder {
    cols: Vec<BuiltColumn>,
}

impl FrameBuilder {
    /// Append a text column (Arrow `Utf8`, dtype `"string"`).
    pub fn text(
        self,
        name: impl Into<String>,
        alias: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<Option<String>>>,
    ) -> Self {
        let array: StringArray = values.into_iter().map(Into::into).collect();
        self.push(name, alias, "string", DataType::Utf8, Arc::new(array))
    }

    /// Append a date column (Arrow `Utf8`, dtype `"date"`). Date cells are
    /// carried as strings, matching the host's existing date handling.
    pub fn date(
        self,
        name: impl Into<String>,
        alias: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<Option<String>>>,
    ) -> Self {
        let array: StringArray = values.into_iter().map(Into::into).collect();
        self.push(name, alias, "date", DataType::Utf8, Arc::new(array))
    }

    /// Append a floating-point column (Arrow `Float64`, dtype `"number"`).
    pub fn number(
        self,
        name: impl Into<String>,
        alias: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<Option<f64>>>,
    ) -> Self {
        let array: Float64Array = values.into_iter().map(Into::into).collect();
        self.push(name, alias, "number", DataType::Float64, Arc::new(array))
    }

    /// Append an integer column (Arrow `Int64`, dtype `"integer"`).
    pub fn integer(
        self,
        name: impl Into<String>,
        alias: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<Option<i64>>>,
    ) -> Self {
        let array: Int64Array = values.into_iter().map(Into::into).collect();
        self.push(name, alias, "integer", DataType::Int64, Arc::new(array))
    }

    /// Append a boolean column (Arrow `Boolean`, dtype `"boolean"`).
    pub fn boolean(
        self,
        name: impl Into<String>,
        alias: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<Option<bool>>>,
    ) -> Self {
        let array: BooleanArray = values.into_iter().map(Into::into).collect();
        self.push(name, alias, "boolean", DataType::Boolean, Arc::new(array))
    }

    fn push(
        mut self,
        name: impl Into<String>,
        alias: impl Into<String>,
        dtype: &str,
        data_type: DataType,
        array: ArrayRef,
    ) -> Self {
        let alias = alias.into();
        let field = Field::new(alias.as_str(), data_type, true);
        self.cols.push(BuiltColumn {
            def: ColumnDef {
                name: name.into(),
                alias,
                dtype: dtype.to_string(),
            },
            field,
            array,
        });
        self
    }

    /// Validate that the columns are rectangular and encode them to one Arrow
    /// IPC file buffer.
    pub fn build(self) -> Result<Frame, Error> {
        let len = self.cols.first().map(|c| c.array.len()).unwrap_or(0);
        for c in &self.cols {
            if c.array.len() != len {
                return Err(Error::RaggedColumns {
                    column: c.def.alias.clone(),
                    expected: len,
                    got: c.array.len(),
                });
            }
        }

        let schema_defs: Vec<ColumnDef> = self.cols.iter().map(|c| c.def.clone()).collect();
        let fields: Vec<Field> = self.cols.iter().map(|c| c.field.clone()).collect();
        let arrays: Vec<ArrayRef> = self.cols.iter().map(|c| Arc::clone(&c.array)).collect();

        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(|e| Error::Arrow(e.to_string()))?;

        let mut ipc = Vec::new();
        {
            let mut writer = FileWriter::try_new(&mut ipc, schema.as_ref()).map_err(|e| Error::Arrow(e.to_string()))?;
            writer.write(&batch).map_err(|e| Error::Arrow(e.to_string()))?;
            writer.finish().map_err(|e| Error::Arrow(e.to_string()))?;
        }

        Ok(Frame {
            schema: schema_defs,
            ipc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emporium_core::tool::data_frame::from_arrow_ipc;

    #[test]
    fn frame_round_trips_through_host_polars_reader() {
        let frame = Frame::builder()
            .text("symbol", "Symbol", ["AAPL".to_string(), "MSFT".to_string()])
            .number("close", "Close", [195.0_f64, 410.5])
            .build()
            .unwrap();

        // Guest wrote arrow-rs IPC; host reads it with polars-io. The whole
        // premise hinges on these two libraries agreeing on the wire.
        let df = from_arrow_ipc(frame.arrow_ipc()).unwrap();
        assert_eq!(df.height(), 2);
        assert_eq!(df.width(), 2);
        assert_eq!(df.get_column_names(), &["Symbol", "Close"]);
    }

    #[test]
    fn computed_floats_survive_bit_exact() {
        // The corruption R1 exists to kill: `base + 0.37*i + 0.4` is not exactly
        // representable, and a JSON decimal round-trip drifts ~6.6% of cells by
        // 1 ULP. Through the SDK → arrow → host path it must be bit-exact.
        let vals: Vec<f64> = (0..256).map(|i| 100.0 + i as f64 * 0.37 + 0.4).collect();
        let frame = Frame::builder().number("v", "v", vals.clone()).build().unwrap();

        let df = from_arrow_ipc(frame.arrow_ipc()).unwrap();
        let col = df.column("v").unwrap().as_materialized_series().f64().unwrap();
        for (i, want) in vals.iter().enumerate() {
            assert_eq!(col.get(i).unwrap().to_bits(), want.to_bits(), "row {i} drifted");
        }
    }

    #[test]
    fn nulls_round_trip() {
        let frame = Frame::builder()
            .number("n", "N", [Some(1.5_f64), None, Some(2.25)])
            .boolean("flag", "Flag", [Some(true), Some(false), None])
            .build()
            .unwrap();

        let df = from_arrow_ipc(frame.arrow_ipc()).unwrap();
        assert_eq!(df.height(), 3);
        let n = df.column("N").unwrap().as_materialized_series().f64().unwrap();
        assert_eq!(n.get(0), Some(1.5));
        assert_eq!(n.get(1), None);
    }

    #[test]
    fn ragged_columns_rejected() {
        let err = Frame::builder()
            .text("a", "A", ["x".to_string()])
            .number("b", "B", [1.0_f64, 2.0])
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::RaggedColumns {
            expected: 1,
            got: 2,
            ..
        }));
    }

    #[test]
    fn schema_carries_name_alias_dtype() {
        let frame = Frame::builder().text("key", "Key", ["a".to_string()]).build().unwrap();
        let schema = frame.schema();
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].name, "key");
        assert_eq!(schema[0].alias, "Key");
        assert_eq!(schema[0].dtype, "string");
    }

    #[test]
    fn escape_hatch_preserves_bytes() {
        let frame = Frame::builder().integer("i", "I", [1_i64, 2, 3]).build().unwrap();
        let (schema, ipc) = frame.into_parts();
        let wrapped = Frame::from_ipc_bytes(schema.clone(), ipc.clone());
        assert_eq!(wrapped.arrow_ipc(), ipc.as_slice());
        assert_eq!(wrapped.schema(), schema.as_slice());
        // And the bytes still decode host-side.
        assert_eq!(from_arrow_ipc(wrapped.arrow_ipc()).unwrap().height(), 3);
    }
}
