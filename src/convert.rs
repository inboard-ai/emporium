//! Conversions from wasmtime-bindgen types to canonical [`emporium_core`] types.
//!
//! These live in the host crate (not emporium-core) per the orphan rule:
//! emporium-core must not depend on wasmtime or any bindgen output. Per
//! MAP_PLAN L279-297 each bindings module (`tool_bindings`, `block_bindings`,
//! `rich_bindings`, ...) generates DISTINCT Rust types for shared interfaces
//! like `tool-provider` and `host-events`, so the From/TryFrom impls are
//! duplicated here by hand — one set per bindings module.
//!
//! Line-count threshold: if Phase 5 adds a fourth bindings module
//! (`full_bindings`) and this file grows past ~500 lines, introduce a
//! declarative macro over the shared tool/block/host-events impl shapes.

use emporium_core::{block, column, event, formula, plan, tool};

use crate::Error;
use crate::wasm::{block_bindings, rich_bindings, tool_bindings};

// ---------------------------------------------------------------------------
// tool-extension world (`tool_bindings`)
// ---------------------------------------------------------------------------

use tool_bindings::emporium::extensions::host_events as wit_host_events;
use tool_bindings::exports::emporium::extensions::tool_provider as wit_tool_provider;

impl From<wit_tool_provider::Activity> for tool::Activity {
    fn from(a: wit_tool_provider::Activity) -> Self {
        tool::Activity {
            present: a.present,
            past: a.past,
            subject_field: a.subject_field,
        }
    }
}

impl TryFrom<wit_tool_provider::ToolInfo> for tool::Info {
    type Error = Error;
    fn try_from(info: wit_tool_provider::ToolInfo) -> Result<Self, Self::Error> {
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
        })
    }
}

impl From<wit_tool_provider::ColumnDef> for column::Def {
    fn from(c: wit_tool_provider::ColumnDef) -> Self {
        column::Def {
            name: c.name,
            alias: c.alias,
            dtype: c.dtype,
        }
    }
}

impl From<wit_tool_provider::TextOutput> for tool::Text {
    fn from(t: wit_tool_provider::TextOutput) -> Self {
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

impl TryFrom<wit_tool_provider::DataFrameOutput> for tool::DataFrame {
    type Error = Error;
    fn try_from(df: wit_tool_provider::DataFrameOutput) -> Result<Self, Self::Error> {
        let schema: Vec<column::Def> = df.schema.into_iter().map(column::Def::from).collect();
        let data: serde_json::Value = if df.rows.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&df.rows)?
        };
        let metadata = match df.metadata {
            Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
            _ => None,
        };
        Ok(tool::DataFrame {
            schema,
            data,
            metadata,
            label: df.label.map(tool::Label::new),
            source: df.source,
            store: df.store,
            description: df.description,
            nickname: df.nickname,
        })
    }
}

impl TryFrom<wit_tool_provider::ToolOutput> for tool::Output {
    type Error = Error;
    fn try_from(out: wit_tool_provider::ToolOutput) -> Result<Self, Self::Error> {
        match out {
            wit_tool_provider::ToolOutput::TextOutput(t) => Ok(tool::Output::Text(t.into())),
            wit_tool_provider::ToolOutput::DataFrameOutput(df) => Ok(tool::Output::DataFrame(df.try_into()?)),
        }
    }
}

impl From<wit_host_events::Progress> for event::Progress {
    fn from(p: wit_host_events::Progress) -> Self {
        event::Progress {
            percent: p.percent,
            message: p.message,
        }
    }
}

impl From<wit_host_events::Invalidate> for event::Invalidate {
    fn from(i: wit_host_events::Invalidate) -> Self {
        match i {
            wit_host_events::Invalidate::Resource(id) => event::Invalidate::Resource(id),
            wit_host_events::Invalidate::Tool(name) => event::Invalidate::Tool(name),
            wit_host_events::Invalidate::All => event::Invalidate::All,
        }
    }
}

impl From<wit_host_events::Event> for event::Event {
    fn from(e: wit_host_events::Event) -> Self {
        match e {
            wit_host_events::Event::Progress(p) => event::Event::Progress(p.into()),
            wit_host_events::Event::ToolsChanged => event::Event::ToolsChanged,
            wit_host_events::Event::Invalidate(i) => event::Event::Invalidate(i.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// block-extension world (`block_bindings`)
// ---------------------------------------------------------------------------

use block_bindings::emporium::extensions::host_events as wit_host_events_block;
use block_bindings::exports::emporium::extensions::{
    block_provider as wit_block_provider, tool_provider as wit_tool_provider_block,
};

impl From<wit_tool_provider_block::Activity> for tool::Activity {
    fn from(a: wit_tool_provider_block::Activity) -> Self {
        tool::Activity {
            present: a.present,
            past: a.past,
            subject_field: a.subject_field,
        }
    }
}

impl TryFrom<wit_tool_provider_block::ToolInfo> for tool::Info {
    type Error = Error;
    fn try_from(info: wit_tool_provider_block::ToolInfo) -> Result<Self, Self::Error> {
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
        })
    }
}

impl From<wit_tool_provider_block::ColumnDef> for column::Def {
    fn from(c: wit_tool_provider_block::ColumnDef) -> Self {
        column::Def {
            name: c.name,
            alias: c.alias,
            dtype: c.dtype,
        }
    }
}

impl From<wit_tool_provider_block::TextOutput> for tool::Text {
    fn from(t: wit_tool_provider_block::TextOutput) -> Self {
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

impl TryFrom<wit_tool_provider_block::DataFrameOutput> for tool::DataFrame {
    type Error = Error;
    fn try_from(df: wit_tool_provider_block::DataFrameOutput) -> Result<Self, Self::Error> {
        let schema: Vec<column::Def> = df.schema.into_iter().map(column::Def::from).collect();
        let data: serde_json::Value = if df.rows.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&df.rows)?
        };
        let metadata = match df.metadata {
            Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
            _ => None,
        };
        Ok(tool::DataFrame {
            schema,
            data,
            metadata,
            label: df.label.map(tool::Label::new),
            source: df.source,
            store: df.store,
            description: df.description,
            nickname: df.nickname,
        })
    }
}

impl TryFrom<wit_tool_provider_block::ToolOutput> for tool::Output {
    type Error = Error;
    fn try_from(out: wit_tool_provider_block::ToolOutput) -> Result<Self, Self::Error> {
        match out {
            wit_tool_provider_block::ToolOutput::TextOutput(t) => Ok(tool::Output::Text(t.into())),
            wit_tool_provider_block::ToolOutput::DataFrameOutput(df) => Ok(tool::Output::DataFrame(df.try_into()?)),
        }
    }
}

impl From<wit_host_events_block::Progress> for event::Progress {
    fn from(p: wit_host_events_block::Progress) -> Self {
        event::Progress {
            percent: p.percent,
            message: p.message,
        }
    }
}

impl From<wit_host_events_block::Invalidate> for event::Invalidate {
    fn from(i: wit_host_events_block::Invalidate) -> Self {
        match i {
            wit_host_events_block::Invalidate::Resource(id) => event::Invalidate::Resource(id),
            wit_host_events_block::Invalidate::Tool(name) => event::Invalidate::Tool(name),
            wit_host_events_block::Invalidate::All => event::Invalidate::All,
        }
    }
}

impl From<wit_host_events_block::Event> for event::Event {
    fn from(e: wit_host_events_block::Event) -> Self {
        match e {
            wit_host_events_block::Event::Progress(p) => event::Event::Progress(p.into()),
            wit_host_events_block::Event::ToolsChanged => event::Event::ToolsChanged,
            wit_host_events_block::Event::Invalidate(i) => event::Event::Invalidate(i.into()),
        }
    }
}

impl From<wit_block_provider::BlockTypeInfo> for block::TypeInfo {
    fn from(info: wit_block_provider::BlockTypeInfo) -> Self {
        block::TypeInfo {
            kind: info.kind,
            name: info.name,
            description: info.description,
        }
    }
}

impl From<wit_block_provider::PlanOutcome> for plan::Outcome {
    fn from(outcome: wit_block_provider::PlanOutcome) -> Self {
        match outcome {
            wit_block_provider::PlanOutcome::StateUpdate(s) => plan::Outcome::StateUpdate(s),
            wit_block_provider::PlanOutcome::Query(q) => plan::Outcome::Query(q),
            wit_block_provider::PlanOutcome::StateUpdateWithQuery((s, q)) => plan::Outcome::StateUpdateWithQuery(s, q),
            wit_block_provider::PlanOutcome::Computed(c) => plan::Outcome::Computed(c),
            wit_block_provider::PlanOutcome::StateUpdateWithComputed((s, c)) => {
                plan::Outcome::StateUpdateWithComputed(s, c)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// rich-extension world (`rich_bindings`)
// ---------------------------------------------------------------------------

use rich_bindings::emporium::extensions::host_events as wit_host_events_rich;
use rich_bindings::exports::emporium::extensions::{
    block_provider as wit_block_provider_rich, formula_provider as wit_formula_provider_rich,
    tool_provider as wit_tool_provider_rich,
};

impl From<wit_tool_provider_rich::Activity> for tool::Activity {
    fn from(a: wit_tool_provider_rich::Activity) -> Self {
        tool::Activity {
            present: a.present,
            past: a.past,
            subject_field: a.subject_field,
        }
    }
}

impl TryFrom<wit_tool_provider_rich::ToolInfo> for tool::Info {
    type Error = Error;
    fn try_from(info: wit_tool_provider_rich::ToolInfo) -> Result<Self, Self::Error> {
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
        })
    }
}

impl From<wit_tool_provider_rich::ColumnDef> for column::Def {
    fn from(c: wit_tool_provider_rich::ColumnDef) -> Self {
        column::Def {
            name: c.name,
            alias: c.alias,
            dtype: c.dtype,
        }
    }
}

impl From<wit_tool_provider_rich::TextOutput> for tool::Text {
    fn from(t: wit_tool_provider_rich::TextOutput) -> Self {
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

impl TryFrom<wit_tool_provider_rich::DataFrameOutput> for tool::DataFrame {
    type Error = Error;
    fn try_from(df: wit_tool_provider_rich::DataFrameOutput) -> Result<Self, Self::Error> {
        let schema: Vec<column::Def> = df.schema.into_iter().map(column::Def::from).collect();
        let data: serde_json::Value = if df.rows.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&df.rows)?
        };
        let metadata = match df.metadata {
            Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
            _ => None,
        };
        Ok(tool::DataFrame {
            schema,
            data,
            metadata,
            label: df.label.map(tool::Label::new),
            source: df.source,
            store: df.store,
            description: df.description,
            nickname: df.nickname,
        })
    }
}

impl TryFrom<wit_tool_provider_rich::ToolOutput> for tool::Output {
    type Error = Error;
    fn try_from(out: wit_tool_provider_rich::ToolOutput) -> Result<Self, Self::Error> {
        match out {
            wit_tool_provider_rich::ToolOutput::TextOutput(t) => Ok(tool::Output::Text(t.into())),
            wit_tool_provider_rich::ToolOutput::DataFrameOutput(df) => Ok(tool::Output::DataFrame(df.try_into()?)),
        }
    }
}

impl From<wit_host_events_rich::Progress> for event::Progress {
    fn from(p: wit_host_events_rich::Progress) -> Self {
        event::Progress {
            percent: p.percent,
            message: p.message,
        }
    }
}

impl From<wit_host_events_rich::Invalidate> for event::Invalidate {
    fn from(i: wit_host_events_rich::Invalidate) -> Self {
        match i {
            wit_host_events_rich::Invalidate::Resource(id) => event::Invalidate::Resource(id),
            wit_host_events_rich::Invalidate::Tool(name) => event::Invalidate::Tool(name),
            wit_host_events_rich::Invalidate::All => event::Invalidate::All,
        }
    }
}

impl From<wit_host_events_rich::Event> for event::Event {
    fn from(e: wit_host_events_rich::Event) -> Self {
        match e {
            wit_host_events_rich::Event::Progress(p) => event::Event::Progress(p.into()),
            wit_host_events_rich::Event::ToolsChanged => event::Event::ToolsChanged,
            wit_host_events_rich::Event::Invalidate(i) => event::Event::Invalidate(i.into()),
        }
    }
}

impl From<wit_block_provider_rich::BlockTypeInfo> for block::TypeInfo {
    fn from(info: wit_block_provider_rich::BlockTypeInfo) -> Self {
        block::TypeInfo {
            kind: info.kind,
            name: info.name,
            description: info.description,
        }
    }
}

impl From<wit_block_provider_rich::PlanOutcome> for plan::Outcome {
    fn from(outcome: wit_block_provider_rich::PlanOutcome) -> Self {
        match outcome {
            wit_block_provider_rich::PlanOutcome::StateUpdate(s) => plan::Outcome::StateUpdate(s),
            wit_block_provider_rich::PlanOutcome::Query(q) => plan::Outcome::Query(q),
            wit_block_provider_rich::PlanOutcome::StateUpdateWithQuery((s, q)) => {
                plan::Outcome::StateUpdateWithQuery(s, q)
            }
            wit_block_provider_rich::PlanOutcome::Computed(c) => plan::Outcome::Computed(c),
            wit_block_provider_rich::PlanOutcome::StateUpdateWithComputed((s, c)) => {
                plan::Outcome::StateUpdateWithComputed(s, c)
            }
        }
    }
}

impl TryFrom<wit_formula_provider_rich::FormulaDef> for formula::Def {
    type Error = Error;
    fn try_from(def: wit_formula_provider_rich::FormulaDef) -> Result<Self, Self::Error> {
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
