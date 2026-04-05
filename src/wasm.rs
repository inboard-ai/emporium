//! WASM extension worker: runtime host for typed provider bindings.
//!
//! A dedicated OS thread owns the wasmtime `Store` (which is `!Send`). The
//! worker spins up a single-threaded tokio runtime inside that thread, calls
//! the extension's `init`, then processes [`Request`]s one at a time and
//! replies via `oneshot` senders embedded in each request.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use emporium_core::{column, event, tool};
use tokio::sync::{broadcast, mpsc, oneshot};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::WasiHttpCtx;
use wasmtime_wasi_http::p2::{WasiHttpCtxView, WasiHttpView};

use crate::Error;
use crate::manifest::Manifest;

/// WIT package namespace and identifier for the emporium extension interfaces.
pub(crate) const WIT_PACKAGE: &str = "emporium:extensions";
/// WIT package version currently supported by the host.
pub(crate) const WIT_VERSION: &str = "0.2.0";

/// Default world selected when a manifest does not declare one.
pub(crate) const DEFAULT_WORLD: &str = "tool-extension";

/// Broadcast capacity for the host-events channel, per MAP_PLAN.
const EVENT_CAPACITY: usize = 256;

/// wasmtime bindings for the `tool-extension` world.
///
/// Bindgen produces `ToolExtension`, `ToolExtensionPre`, the host-events
/// `Host` trait, and the `exports::emporium::extensions::*` type records.
#[allow(clippy::all)]
pub(crate) mod tool_bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "tool-extension",
        imports: { default: async },
        exports: { default: async },
    });
}

use tool_bindings::ToolExtension;
use tool_bindings::emporium::extensions::host_events as wit_host_events;
use tool_bindings::exports::emporium::extensions::tool_provider as wit_tool_provider;

/// Handle returned from [`spawn_worker`]: gives the caller the means to send
/// requests and subscribe to events. The worker thread keeps running as long
/// as any clone of `requests` survives.
pub(crate) struct Worker {
    /// Sender of typed worker requests.
    pub(crate) requests: mpsc::UnboundedSender<Request>,
    /// Broadcast sender for host-events notifications.
    pub(crate) events: broadcast::Sender<event::Event>,
    /// Cached metadata captured during `extension::init`.
    pub(crate) metadata: tool_bindings::exports::emporium::extensions::extension::Metadata,
}

/// A single typed request dispatched to the worker thread.
///
/// Every variant embeds an `oneshot::Sender` for its reply. Dropping the
/// sender signals the caller that the worker has crashed.
pub(crate) enum Request {
    /// Request the current list of tools.
    ListTools {
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<Vec<tool::Info>, Error>>,
    },
    /// Execute a named tool with JSON parameters.
    ExecuteTool {
        /// Tool identifier (matches `tool::Info.id`).
        name: String,
        /// JSON-encoded parameters.
        params: String,
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<tool::Output, Error>>,
    },
    /// Retrieve the extension's current rendered view.
    View {
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<String, Error>>,
    },
}

/// Per-store state for wasmtime: WASI context, HTTP context, a resource
/// table, and the host-events broadcast sender.
pub(crate) struct State {
    wasi: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
    events: broadcast::Sender<event::Event>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

impl wit_host_events::Host for State {
    async fn notify(&mut self, event: wit_host_events::Event) {
        let core_event: event::Event = event.into();
        tracing::info!(?core_event, "host-events: received notify from extension");
        // Ignore SendError: "no subscribers" is expected during init.
        let _ = self.events.send(core_event);
    }
}

/// Helper used by bindgen's `add_to_linker` — projects `&mut State` into the
/// host-events handler's associated `Data<'a>` type.
struct HostEventsData;

impl wasmtime::component::HasData for HostEventsData {
    type Data<'a> = &'a mut State;
}

/// Spawn the OS-thread worker. The caller receives a [`Worker`] handle after
/// the extension's `init` completes.
///
/// Errors from `init` are surfaced synchronously through this function's
/// return value; the worker thread exits in that case. If the thread panics
/// before it can report metadata, the caller observes
/// [`Error::ThreadDiedDuringInit`].
pub(crate) async fn spawn_worker(
    manifest: Arc<Manifest>,
    wasm_bytes: Vec<u8>,
    config: String,
) -> Result<Worker, Error> {
    let world = manifest.world().to_string();
    if world != DEFAULT_WORLD {
        return Err(Error::UnsupportedWorld(world));
    }

    let (meta_tx, meta_rx) = oneshot::channel::<Result<WorkerBootstrap, Error>>();
    let (req_tx, req_rx) = mpsc::unbounded_channel::<Request>();
    let (event_tx, _) = broadcast::channel::<event::Event>(EVENT_CAPACITY);
    let event_tx_thread = event_tx.clone();

    std::thread::Builder::new()
        .name(format!("emporium-worker:{}", manifest.id))
        .spawn(move || {
            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = meta_tx.send(Err(Error::Io(Arc::new(err))));
                        return;
                    }
                };
                runtime.block_on(run_tool_worker(wasm_bytes, config, event_tx_thread, meta_tx, req_rx));
            }));
            if let Err(payload) = outcome {
                tracing::error!(?payload, "emporium worker panicked");
            }
        })
        .map_err(|err| Error::Io(Arc::new(err)))?;

    let bootstrap = match meta_rx.await {
        Ok(Ok(b)) => b,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err(Error::ThreadDiedDuringInit),
    };

    Ok(Worker {
        requests: req_tx,
        events: event_tx,
        metadata: bootstrap.metadata,
    })
}

/// Internal payload delivered through the meta oneshot on successful init.
struct WorkerBootstrap {
    metadata: tool_bindings::exports::emporium::extensions::extension::Metadata,
}

/// The async body that runs entirely on the worker thread's current-thread
/// tokio runtime. Performs the full bring-up then enters the request loop.
async fn run_tool_worker(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
    meta_tx: oneshot::Sender<Result<WorkerBootstrap, Error>>,
    mut req_rx: mpsc::UnboundedReceiver<Request>,
) {
    // Fused init sequence: on any failure, send Err through meta_tx and exit.
    let init = match initialize(wasm_bytes, config, event_tx).await {
        Ok(ok) => ok,
        Err(err) => {
            let _ = meta_tx.send(Err(err));
            return;
        }
    };
    let Initialized {
        bindings,
        mut store,
        metadata,
    } = init;

    if meta_tx.send(Ok(WorkerBootstrap { metadata })).is_err() {
        // The caller dropped the meta receiver — shut down cleanly.
        return;
    }

    while let Some(request) = req_rx.recv().await {
        handle_request(request, &bindings, &mut store).await;
    }
}

/// Output of the initialization phase.
struct Initialized {
    bindings: ToolExtension,
    store: Store<State>,
    metadata: tool_bindings::exports::emporium::extensions::extension::Metadata,
}

/// Compile, instantiate, `init`, and capture metadata.
async fn initialize(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
) -> Result<Initialized, Error> {
    let mut cfg = Config::new();
    cfg.wasm_component_model(true);
    let engine = Engine::new(&cfg)?;
    let component = Component::new(&engine, &wasm_bytes)?;

    validate_tool_world_exports(&engine, &component)?;

    let linker = build_tool_linker(&engine)?;

    let wasi = WasiCtxBuilder::new().inherit_stderr().inherit_stdout().build();
    let state = State {
        wasi,
        http: WasiHttpCtx::new(),
        table: ResourceTable::new(),
        events: event_tx,
    };
    let mut store = Store::new(&engine, state);

    let bindings = ToolExtension::instantiate_async(&mut store, &component, &linker).await?;

    // Fetch metadata first (stateless), then call init.
    let ext_guest = bindings.emporium_extensions_extension();
    let metadata = ext_guest.call_get_metadata(&mut store).await?;
    let init_result = ext_guest.call_init(&mut store, &config).await?;
    if let Err(msg) = init_result {
        return Err(Error::ExtensionLoadError(msg));
    }

    Ok(Initialized {
        bindings,
        store,
        metadata,
    })
}

/// Register WASI, WASI-HTTP, and the host-events interface.
fn build_tool_linker(engine: &Engine) -> Result<Linker<State>, Error> {
    let mut linker = build_base_linker(engine)?;
    ToolExtension::add_to_linker::<State, HostEventsData>(&mut linker, |s| s)?;
    Ok(linker)
}

/// Base linker shared by all worlds: WASI + WASI-HTTP. Each world-specific
/// linker is built on top.
fn build_base_linker(engine: &Engine) -> Result<Linker<State>, Error> {
    let mut linker = Linker::<State>::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    Ok(linker)
}

/// Strict-match validation for the `tool-extension` world. Exports must
/// contain exactly `extension` and `tool-provider`, versioned at
/// [`WIT_VERSION`], and nothing else.
fn validate_tool_world_exports(engine: &Engine, component: &Component) -> Result<(), Error> {
    let declared = DEFAULT_WORLD.to_string();
    let required: Vec<String> = ["extension", "tool-provider"]
        .iter()
        .map(|name| format!("{WIT_PACKAGE}/{name}@{WIT_VERSION}"))
        .collect();

    let actual: Vec<String> = component
        .component_type()
        .exports(engine)
        .map(|(name, _)| name.to_string())
        .collect();

    let missing: Vec<String> = required
        .iter()
        .filter(|name| !actual.iter().any(|n| n == *name))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(Error::WorldMissingExports { declared, missing });
    }

    let extra: Vec<String> = actual
        .into_iter()
        .filter(|name| !required.iter().any(|n| n == name))
        .collect();
    if !extra.is_empty() {
        return Err(Error::WorldExtraExports { declared, extra });
    }

    Ok(())
}

/// Dispatch a single typed [`Request`] to the wasmtime bindings and reply.
async fn handle_request(request: Request, bindings: &ToolExtension, store: &mut Store<State>) {
    match request {
        Request::ListTools { reply } => {
            let outcome = list_tools(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::ExecuteTool { name, params, reply } => {
            let outcome = execute_tool(bindings, store, &name, &params).await;
            let _ = reply.send(outcome);
        }
        Request::View { reply } => {
            let outcome = view(bindings, store).await;
            let _ = reply.send(outcome);
        }
    }
}

async fn list_tools(bindings: &ToolExtension, store: &mut Store<State>) -> Result<Vec<tool::Info>, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_tools = guest.call_list_tools(&mut *store).await?;
    wit_tools
        .into_iter()
        .map(tool::Info::try_from)
        .collect::<Result<Vec<_>, _>>()
}

async fn execute_tool(
    bindings: &ToolExtension,
    store: &mut Store<State>,
    name: &str,
    params: &str,
) -> Result<tool::Output, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_outcome = guest.call_execute_tool(&mut *store, name, params).await?;
    let wit_output = wit_outcome.map_err(Error::ExtensionError)?;
    tool::Output::try_from(wit_output)
}

async fn view(bindings: &ToolExtension, store: &mut Store<State>) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_extension();
    let s = guest.call_view(&mut *store).await?;
    Ok(s)
}

// ---------------------------------------------------------------------------
// Conversions from wasmtime-bindgen types to emporium-core types.
//
// These live in the host crate (not emporium-core) per the orphan rule:
// emporium-core must not depend on wasmtime or any bindgen output.
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_tool_info_round_trip() {
        let schema_json = r#"{"type":"object","properties":{"key":{"type":"string"}}}"#;
        let wit = wit_tool_provider::ToolInfo {
            id: "get".into(),
            name: "Get".into(),
            description: "Fetches a value by key".into(),
            schema: schema_json.into(),
            cacheable: true,
            activity: Some(wit_tool_provider::Activity {
                present: "Getting".into(),
                past: "Got".into(),
                subject_field: "/key".into(),
            }),
        };

        let info = tool::Info::try_from(wit).unwrap();
        assert_eq!(info.id, "get");
        assert_eq!(info.name, "Get");
        assert_eq!(info.description, "Fetches a value by key");
        assert!(info.cacheable);
        assert_eq!(
            info.schema,
            serde_json::from_str::<serde_json::Value>(schema_json).unwrap()
        );
        let activity = info.activity.unwrap();
        assert_eq!(activity.present, "Getting");
        assert_eq!(activity.past, "Got");
        assert_eq!(activity.subject_field, "/key");
    }

    #[test]
    fn from_tool_info_with_empty_schema_yields_null() {
        let wit = wit_tool_provider::ToolInfo {
            id: "noop".into(),
            name: "Noop".into(),
            description: "".into(),
            schema: "".into(),
            cacheable: false,
            activity: None,
        };
        let info = tool::Info::try_from(wit).unwrap();
        assert_eq!(info.schema, serde_json::Value::Null);
        assert!(info.activity.is_none());
        assert!(!info.cacheable);
    }

    #[test]
    fn from_tool_output_round_trip() {
        let wit_text = wit_tool_provider::ToolOutput::TextOutput(wit_tool_provider::TextOutput {
            content: "hello".into(),
            label: Some("greeting".into()),
            source: Some("test".into()),
            description: Some("a greeting".into()),
            nickname: Some("hi".into()),
            store: Some(true),
        });

        let out = tool::Output::try_from(wit_text).unwrap();
        match out {
            tool::Output::Text(t) => {
                assert_eq!(t.content, "hello");
                assert_eq!(t.label.as_ref().map(|l| l.0.as_str()), Some("greeting"));
                assert_eq!(t.source.as_deref(), Some("test"));
                assert_eq!(t.description.as_deref(), Some("a greeting"));
                assert_eq!(t.nickname.as_deref(), Some("hi"));
                assert_eq!(t.store, Some(true));
            }
            _ => panic!("expected Text variant"),
        }

        let wit_df = wit_tool_provider::ToolOutput::DataFrameOutput(wit_tool_provider::DataFrameOutput {
            schema: vec![wit_tool_provider::ColumnDef {
                name: "k".into(),
                alias: "Key".into(),
                dtype: "string".into(),
            }],
            rows: r#"[["a"],["b"]]"#.into(),
            metadata: Some(r#"{"source":"unit"}"#.into()),
            label: Some("kv".into()),
            source: Some("unit".into()),
            description: None,
            nickname: Some("kv_rows".into()),
            store: Some(false),
        });

        let out = tool::Output::try_from(wit_df).unwrap();
        match out {
            tool::Output::DataFrame(df) => {
                assert_eq!(df.schema.len(), 1);
                assert_eq!(df.schema[0].name, "k");
                assert_eq!(df.schema[0].alias, "Key");
                assert_eq!(df.schema[0].dtype, "string");
                assert_eq!(df.data, serde_json::json!([["a"], ["b"]]));
                assert_eq!(df.metadata, Some(serde_json::json!({"source": "unit"})));
                assert_eq!(df.label.as_ref().map(|l| l.0.as_str()), Some("kv"));
                assert_eq!(df.source.as_deref(), Some("unit"));
                assert_eq!(df.description, None);
                assert_eq!(df.nickname.as_deref(), Some("kv_rows"));
                assert_eq!(df.store, Some(false));
            }
            _ => panic!("expected DataFrame variant"),
        }
    }

    #[test]
    fn from_event_covers_all_variants() {
        let prog = wit_host_events::Event::Progress(wit_host_events::Progress {
            percent: Some(50),
            message: "halfway".into(),
        });
        match event::Event::from(prog) {
            event::Event::Progress(p) => {
                assert_eq!(p.percent, Some(50));
                assert_eq!(p.message, "halfway");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match event::Event::from(wit_host_events::Event::ToolsChanged) {
            event::Event::ToolsChanged => {}
            other => panic!("unexpected event: {other:?}"),
        }

        let inv = wit_host_events::Event::Invalidate(wit_host_events::Invalidate::Tool("get".into()));
        match event::Event::from(inv) {
            event::Event::Invalidate(event::Invalidate::Tool(name)) => assert_eq!(name, "get"),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
