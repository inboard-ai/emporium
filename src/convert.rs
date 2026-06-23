//! Conversions from wasmtime-bindgen types to canonical [`emporium_core`] types.
//!
//! These live in the host crate (not emporium-core) per the orphan rule:
//! emporium-core must not depend on wasmtime or any bindgen output. Each
//! bindings module (`tool_bindings`, `block_bindings`, `rich_bindings`,
//! `full_bindings`) generates DISTINCT Rust types for shared interfaces like
//! `tool-provider` and `host-events`, so the From/TryFrom impls are stamped
//! out with declarative macros below — one invocation per bindings module.

use emporium_core::{block, column, data, event, formula, plan, tool};

use crate::Error;
use crate::wasm::{block_bindings, data_bindings, full_bindings, rich_bindings, tool_bindings};

// ---------------------------------------------------------------------------
// Macro: tool-provider + host-events impls (all worlds)
// ---------------------------------------------------------------------------

/// Generates the 9 From/TryFrom impls that convert tool-provider and
/// host-events WIT types from a single bindings module to emporium-core types.
///
/// Parameters:
///   $tool_mod — use-alias for the `tool_provider` module (exports)
///   $col_mod  — use-alias for the module whose `ColumnDef` is used by
///               `DataFrameOutput` (same as tool_provider for tool/block/rich;
///               `types` for full)
///   $evt_mod  — use-alias for the `host_events` module (imports)
macro_rules! impl_tool_and_events {
    ($tool_mod:ident, $col_mod:ident, $evt_mod:ident) => {
        // -- tool_provider types ------------------------------------------

        impl From<$tool_mod::Activity> for tool::Activity {
            fn from(a: $tool_mod::Activity) -> Self {
                tool::Activity {
                    present: a.present,
                    past: a.past,
                    subject_field: a.subject_field,
                }
            }
        }

        impl TryFrom<$tool_mod::ToolInfo> for tool::Info {
            type Error = Error;
            fn try_from(info: $tool_mod::ToolInfo) -> Result<Self, Self::Error> {
                let schema: serde_json::Value = if info.schema.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&info.schema)?
                };
                Ok(tool::Info {
                    id: info.id,
                    name: info.name,
                    description: info.description,
                    schema,
                    cacheable: info.cacheable,
                    activity: info.activity.map(tool::Activity::from),
                    examples: info
                        .examples
                        .into_iter()
                        .map(|s| serde_json::from_str(&s))
                        .collect::<Result<Vec<_>, _>>()?,
                })
            }
        }

        impl From<$col_mod::ColumnDef> for column::Def {
            fn from(c: $col_mod::ColumnDef) -> Self {
                // The wire carries bare strings; lift them into the distinct
                // name/alias newtypes so they can't be swapped host-side.
                column::Def {
                    name: column::Name::new(c.name),
                    alias: column::Alias::new(c.alias),
                    dtype: c.dtype,
                }
            }
        }

        impl From<$tool_mod::TextOutput> for tool::Text {
            fn from(t: $tool_mod::TextOutput) -> Self {
                tool::Text {
                    content: t.content,
                    label: t.label.map(tool::Label::new),
                    source: t.source,
                    store: t.store,
                    description: t.description,
                    nickname: t.nickname,
                }
            }
        }

        impl TryFrom<$tool_mod::DataFrameOutput> for tool::DataFrame {
            type Error = Error;
            fn try_from(df: $tool_mod::DataFrameOutput) -> Result<Self, Self::Error> {
                let schema: Vec<column::Def> = df.schema.into_iter().map(column::Def::from).collect();
                // The data crosses as an Arrow IPC buffer — stored as-is, with no
                // JSON parse and no per-cell allocation. Decoded lazily host-side
                // via `tool::DataFrame::to_dataframe`. (R1 wire swap.)
                let metadata = match df.metadata {
                    Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
                    _ => None,
                };
                Ok(tool::DataFrame {
                    schema,
                    frame_ipc: df.arrow_ipc,
                    metadata,
                    label: df.label.map(tool::Label::new),
                    source: df.source,
                    store: df.store,
                    description: df.description,
                    nickname: df.nickname,
                })
            }
        }

        impl TryFrom<$tool_mod::ToolOutput> for tool::Output {
            type Error = Error;
            fn try_from(out: $tool_mod::ToolOutput) -> Result<Self, Self::Error> {
                match out {
                    $tool_mod::ToolOutput::TextOutput(t) => Ok(tool::Output::Text(t.into())),
                    $tool_mod::ToolOutput::DataFrameOutput(df) => Ok(tool::Output::DataFrame(df.try_into()?)),
                }
            }
        }

        // -- host_events types --------------------------------------------

        impl From<$evt_mod::Progress> for event::Progress {
            fn from(p: $evt_mod::Progress) -> Self {
                event::Progress {
                    percent: p.percent,
                    message: p.message,
                }
            }
        }

        impl From<$evt_mod::Invalidate> for event::Invalidate {
            fn from(i: $evt_mod::Invalidate) -> Self {
                match i {
                    $evt_mod::Invalidate::Resource(id) => event::Invalidate::Resource(id),
                    $evt_mod::Invalidate::Tool(name) => event::Invalidate::Tool(name),
                    $evt_mod::Invalidate::All => event::Invalidate::All,
                }
            }
        }

        impl From<$evt_mod::Event> for event::Event {
            fn from(e: $evt_mod::Event) -> Self {
                match e {
                    $evt_mod::Event::Progress(p) => event::Event::Progress(p.into()),
                    $evt_mod::Event::ToolsChanged => event::Event::ToolsChanged,
                    $evt_mod::Event::Invalidate(i) => event::Event::Invalidate(i.into()),
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Macro: block-provider impls (block, rich, full worlds)
// ---------------------------------------------------------------------------

/// Generates the 2 From impls that convert block-provider WIT types to
/// emporium-core types.
///
/// Parameters:
///   $block_mod — use-alias for the `block_provider` module (exports)
macro_rules! impl_block {
    ($block_mod:ident) => {
        impl From<$block_mod::BlockTypeInfo> for block::TypeInfo {
            fn from(info: $block_mod::BlockTypeInfo) -> Self {
                block::TypeInfo {
                    kind: info.kind,
                    name: info.name,
                    description: info.description,
                }
            }
        }

        impl From<$block_mod::PlanOutcome> for plan::Outcome {
            fn from(outcome: $block_mod::PlanOutcome) -> Self {
                match outcome {
                    $block_mod::PlanOutcome::StateUpdate(s) => plan::Outcome::StateUpdate(s),
                    $block_mod::PlanOutcome::Query(q) => plan::Outcome::Query(q),
                    $block_mod::PlanOutcome::StateUpdateWithQuery((s, q)) => plan::Outcome::StateUpdateWithQuery(s, q),
                    $block_mod::PlanOutcome::Computed(c) => plan::Outcome::Computed(c),
                    $block_mod::PlanOutcome::StateUpdateWithComputed((s, c)) => {
                        plan::Outcome::StateUpdateWithComputed(s, c)
                    }
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Macro: formula-provider impls (rich, full worlds)
// ---------------------------------------------------------------------------

/// Generates the TryFrom impl that converts formula-provider WIT types to
/// emporium-core types.
///
/// Parameters:
///   $formula_mod — use-alias for the `formula_provider` module (exports)
macro_rules! impl_formula {
    ($formula_mod:ident) => {
        impl TryFrom<$formula_mod::FormulaDef> for formula::Def {
            type Error = Error;
            fn try_from(def: $formula_mod::FormulaDef) -> Result<Self, Self::Error> {
                let schema: serde_json::Value = if def.schema.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&def.schema)?
                };
                Ok(formula::Def {
                    name: def.name,
                    description: def.description,
                    schema,
                })
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Invocations — one section per bindings module / world
// ---------------------------------------------------------------------------

// tool-extension world: tool + host-events only
use tool_bindings::emporium::extensions::host_events as tool_evt;
use tool_bindings::exports::emporium::extensions::tool_provider as tool_tp;

impl_tool_and_events!(tool_tp, tool_tp, tool_evt);

// block-extension world: tool + host-events + block
use block_bindings::emporium::extensions::host_events as block_evt;
use block_bindings::exports::emporium::extensions::{block_provider as block_bp, tool_provider as block_tp};

impl_tool_and_events!(block_tp, block_tp, block_evt);
impl_block!(block_bp);

// rich-extension world: tool + host-events + block + formula
use rich_bindings::emporium::extensions::host_events as rich_evt;
use rich_bindings::exports::emporium::extensions::{
    block_provider as rich_bp, formula_provider as rich_fp, tool_provider as rich_tp,
};

impl_tool_and_events!(rich_tp, rich_tp, rich_evt);
impl_block!(rich_bp);
impl_formula!(rich_fp);

// full-extension world: tool + host-events + block + formula + types
// NOTE: ColumnDef lives under `types` (not `tool_provider`) in the full world
// because the `host-data` import also uses it.
use full_bindings::emporium::extensions::{host_events as full_evt, types as full_types};
use full_bindings::exports::emporium::extensions::{
    block_provider as full_bp, formula_provider as full_fp, tool_provider as full_tp,
};

impl_tool_and_events!(full_tp, full_types, full_evt);
impl_block!(full_bp);
impl_formula!(full_fp);

// ---------------------------------------------------------------------------
// full-extension: reverse ColumnDef conversion (host -> guest)
// ---------------------------------------------------------------------------

// Reverse conversion for the host-side `host-data::get-schema` return value:
// the host owns the canonical `column::Def` and hands it back over the WIT
// boundary as the bindgen-generated `ColumnDef` record. This impl is unique
// to full-extension (only world that imports `host-data`).
impl From<column::Def> for full_types::ColumnDef {
    fn from(c: column::Def) -> Self {
        // Lower the typed identities back to bare strings for the WIT wire.
        full_types::ColumnDef {
            name: c.name.into_string(),
            alias: c.alias.into_string(),
            dtype: c.dtype,
        }
    }
}

// ---------------------------------------------------------------------------
// data-extension world: tool + host-events + data-provider
// ---------------------------------------------------------------------------

// data-extension world: tool + host-events (reuse the shared macro).
// NOTE: like full-extension, `column-def` lives under `types` (not
// `tool_provider`) because the `data-provider` export also references it, so
// bindgen lifts it to the shared `types` interface module.
use data_bindings::emporium::extensions::{host_events as data_evt, types as data_types};
use data_bindings::exports::emporium::extensions::{data_provider as data_dp, tool_provider as data_tp};

impl_tool_and_events!(data_tp, data_types, data_evt);

// -- data-provider types: WIT -> canonical emporium_core::data --------------

impl From<data_dp::RateLimit> for data::RateLimit {
    fn from(r: data_dp::RateLimit) -> Self {
        data::RateLimit {
            message: r.message,
            retry_after_secs: r.retry_after_secs,
        }
    }
}

impl From<data_dp::DataError> for data::Error {
    fn from(e: data_dp::DataError) -> Self {
        match e {
            data_dp::DataError::AuthFailed(s) => data::Error::AuthFailed(s),
            data_dp::DataError::NotFound(s) => data::Error::NotFound(s),
            data_dp::DataError::InvalidSelection(s) => data::Error::InvalidSelection(s),
            data_dp::DataError::RateLimited(r) => data::Error::RateLimited(r.into()),
            data_dp::DataError::Transient(s) => data::Error::Transient(s),
            data_dp::DataError::Unsupported(s) => data::Error::Unsupported(s),
            data_dp::DataError::Internal(s) => data::Error::Internal(s),
        }
    }
}

impl TryFrom<data_dp::OutputSpec> for data::source::Shape {
    type Error = Error;
    fn try_from(spec: data_dp::OutputSpec) -> Result<Self, Self::Error> {
        Ok(match spec {
            data_dp::OutputSpec::Rows(cols) => {
                data::source::Shape::Rows(cols.into_iter().map(column::Def::from).collect())
            }
            data_dp::OutputSpec::Scalar(col) => data::source::Shape::Scalar(col.into()),
            data_dp::OutputSpec::Finished => data::source::Shape::Finished,
        })
    }
}

impl From<data_dp::FreeQuerySpec> for data::source::FreeQuery {
    fn from(s: data_dp::FreeQuerySpec) -> Self {
        data::source::FreeQuery {
            language: s.language,
            placeholder: s.placeholder,
        }
    }
}

impl TryFrom<data_dp::InputSpec> for data::source::Input {
    type Error = Error;
    fn try_from(spec: data_dp::InputSpec) -> Result<Self, Self::Error> {
        Ok(match spec {
            data_dp::InputSpec::Params(s) => data::source::Input::Params(serde_json::from_str(&s)?),
            data_dp::InputSpec::Browse => data::source::Input::Browse,
            data_dp::InputSpec::ParamsAndBrowse(s) => data::source::Input::ParamsAndBrowse(serde_json::from_str(&s)?),
            data_dp::InputSpec::FreeQuery(q) => data::source::Input::FreeQuery(q.into()),
        })
    }
}

impl TryFrom<data_dp::OutputContract> for data::source::Output {
    type Error = Error;
    fn try_from(contract: data_dp::OutputContract) -> Result<Self, Self::Error> {
        Ok(match contract {
            data_dp::OutputContract::Known(spec) => data::source::Output::Known(spec.try_into()?),
            data_dp::OutputContract::Resolved => data::source::Output::Resolved,
        })
    }
}

impl TryFrom<data_dp::DataSource> for data::Source {
    type Error = Error;
    fn try_from(src: data_dp::DataSource) -> Result<Self, Self::Error> {
        Ok(data::Source {
            id: src.id,
            display_name: src.display_name,
            description: src.description,
            input: src.input.try_into()?,
            output: src.output.try_into()?,
            cardinality: src.cardinality.into(),
        })
    }
}

impl From<data_dp::Cardinality> for data::source::Cardinality {
    fn from(c: data_dp::Cardinality) -> Self {
        match c {
            data_dp::Cardinality::Rows => data::source::Cardinality::Rows,
            data_dp::Cardinality::SingleRecord => data::source::Cardinality::Record,
            data_dp::Cardinality::Scalar => data::source::Cardinality::Scalar,
        }
    }
}

impl TryFrom<data_dp::Node> for data::Node {
    type Error = Error;
    fn try_from(node: data_dp::Node) -> Result<Self, Self::Error> {
        Ok(data::Node {
            id: node.id,
            label: node.label,
            branch: node.branch,
            output: node.output.map(data::source::Shape::try_from).transpose()?,
        })
    }
}

impl TryFrom<data_dp::Page> for data::Page {
    type Error = Error;
    fn try_from(page: data_dp::Page) -> Result<Self, Self::Error> {
        Ok(data::Page {
            nodes: page
                .nodes
                .into_iter()
                .map(data::Node::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            next_cursor: page.next_cursor,
        })
    }
}

impl From<data_dp::FetchResult> for data::FetchResult {
    fn from(r: data_dp::FetchResult) -> Self {
        data::FetchResult {
            arrow_ipc: r.arrow_ipc,
            resource_id: r.resource_id,
            next_cursor: r.next_cursor,
        }
    }
}
