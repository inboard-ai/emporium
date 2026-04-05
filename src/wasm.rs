//! WASM extension worker: runtime host for typed provider bindings.
//!
//! A dedicated OS thread owns the wasmtime `Store` (which is `!Send`). The
//! worker spins up a single-threaded tokio runtime inside that thread, calls
//! the extension's `init`, then processes [`Request`]s one at a time and
//! replies via `oneshot` senders embedded in each request.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use emporium_core::{block, event, formula, plan, tool};
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

/// World name for the block-extension capability tier.
pub(crate) const BLOCK_WORLD: &str = "block-extension";

/// World name for the rich-extension capability tier (block + formula).
pub(crate) const RICH_WORLD: &str = "rich-extension";

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

/// wasmtime bindings for the `block-extension` world.
///
/// Bindgen produces `BlockExtension`, `BlockExtensionPre`, the host-events
/// `Host` trait, and the `exports::emporium::extensions::*` type records —
/// including the block-provider types that `tool_bindings` does not contain.
/// The shared types (`Metadata`, `ToolInfo`, etc.) are regenerated here as
/// DISTINCT Rust types from their `tool_bindings` counterparts.
#[allow(clippy::all)]
pub(crate) mod block_bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "block-extension",
        imports: { default: async },
        exports: { default: async },
    });
}

use block_bindings::BlockExtension;
use block_bindings::emporium::extensions::host_events as wit_host_events_block;

/// wasmtime bindings for the `rich-extension` world.
///
/// Bindgen produces `RichExtension`, `RichExtensionPre`, the host-events
/// `Host` trait, and the `exports::emporium::extensions::*` type records —
/// including the formula-provider types that neither `tool_bindings` nor
/// `block_bindings` contain. Like those other modules, the shared types are
/// regenerated here as DISTINCT Rust types from their counterparts.
#[allow(clippy::all)]
pub(crate) mod rich_bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "rich-extension",
        imports: { default: async },
        exports: { default: async },
    });
}

use rich_bindings::RichExtension;
use rich_bindings::emporium::extensions::host_events as wit_host_events_rich;

/// Handle returned from [`spawn_worker`]: gives the caller the means to send
/// requests and subscribe to events. The worker thread keeps running as long
/// as any clone of `requests` survives.
pub(crate) struct Worker {
    /// Sender of typed worker requests.
    pub(crate) requests: mpsc::UnboundedSender<Request>,
    /// Broadcast sender for host-events notifications.
    pub(crate) events: broadcast::Sender<event::Event>,
    /// Cached metadata captured during `extension::init`. Canonical core type
    /// so the extension crate need not care which bindings module produced it.
    pub(crate) metadata: WorkerMetadata,
    /// True when the extension was loaded against a world that exports
    /// `block-provider`. Gates [`Extension::block`](crate::Extension::block).
    pub(crate) has_block: bool,
    /// True when the extension was loaded against a world that exports
    /// `formula-provider`. Gates [`Extension::formula`](crate::Extension::formula).
    pub(crate) has_formula: bool,
}

/// Metadata captured from `extension::get-metadata` at init, in a
/// bindings-independent form so both worker variants can populate it.
#[derive(Debug, Clone)]
pub(crate) struct WorkerMetadata {
    /// Extension identifier.
    pub(crate) id: String,
    /// Human-readable extension name.
    pub(crate) name: String,
    /// Version reported by the component itself.
    pub(crate) version: String,
    /// Description reported by the component itself.
    pub(crate) description: String,
}

impl From<tool_bindings::exports::emporium::extensions::extension::Metadata> for WorkerMetadata {
    fn from(m: tool_bindings::exports::emporium::extensions::extension::Metadata) -> Self {
        WorkerMetadata {
            id: m.id,
            name: m.name,
            version: m.version,
            description: m.description,
        }
    }
}

impl From<block_bindings::exports::emporium::extensions::extension::Metadata> for WorkerMetadata {
    fn from(m: block_bindings::exports::emporium::extensions::extension::Metadata) -> Self {
        WorkerMetadata {
            id: m.id,
            name: m.name,
            version: m.version,
            description: m.description,
        }
    }
}

impl From<rich_bindings::exports::emporium::extensions::extension::Metadata> for WorkerMetadata {
    fn from(m: rich_bindings::exports::emporium::extensions::extension::Metadata) -> Self {
        WorkerMetadata {
            id: m.id,
            name: m.name,
            version: m.version,
            description: m.description,
        }
    }
}

/// A single typed request dispatched to the worker thread.
///
/// Every variant embeds an `oneshot::Sender` for its reply. Dropping the
/// sender signals the caller that the worker has crashed.
///
/// The `Tool*` / `View` variants are handled by every world; `Block*`
/// variants are only handled by block-capable workers (block + rich);
/// `Formula*` variants are only handled by the rich-extension worker. A
/// worker receiving a variant its world does not cover replies with
/// [`Error::UnsupportedWorld`] — this is a defensive guard; in practice
/// [`Extension::block`](crate::Extension::block) and
/// [`Extension::formula`](crate::Extension::formula) only hand out providers
/// when the worker was loaded against a world that exports them.
#[non_exhaustive]
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
    /// Enumerate the block types this extension exports.
    BlockTypes {
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<Vec<block::TypeInfo>, Error>>,
    },
    /// Ask the extension to initialize a new block of `kind` from JSON input
    /// and return the initial state as a JSON string.
    BlockCreate {
        /// Block kind identifier.
        kind: String,
        /// JSON-encoded creation parameters.
        input: String,
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<String, Error>>,
    },
    /// Run a block operation against existing state; returns the plan outcome.
    BlockPlan {
        /// Block kind identifier.
        kind: String,
        /// JSON-encoded prior state.
        state: String,
        /// Operation name.
        operation: String,
        /// JSON-encoded operation parameters.
        input: String,
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<plan::Outcome, Error>>,
    },
    /// Ask the extension to validate an encoded block state.
    BlockValidate {
        /// Block kind identifier.
        kind: String,
        /// JSON-encoded state to validate.
        state: String,
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<(), Error>>,
    },
    /// Request the current list of formula definitions.
    FormulaDefs {
        /// Channel used to deliver the reply.
        reply: oneshot::Sender<Result<Vec<formula::Def>, Error>>,
    },
    /// Evaluate a named formula.
    FormulaEvaluate {
        /// Formula name (matches `formula::Def.name`).
        name: String,
        /// JSON-encoded args (typically a JSON array of positional arguments).
        args: String,
        /// JSON-encoded result (whatever shape the formula returns).
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

impl wit_host_events_block::Host for State {
    async fn notify(&mut self, event: wit_host_events_block::Event) {
        let core_event: event::Event = event.into();
        tracing::info!(?core_event, "host-events: received notify from extension");
        let _ = self.events.send(core_event);
    }
}

impl wit_host_events_rich::Host for State {
    async fn notify(&mut self, event: wit_host_events_rich::Event) {
        let core_event: event::Event = event.into();
        tracing::info!(?core_event, "host-events: received notify from extension");
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
    let route = match world.as_str() {
        DEFAULT_WORLD => WorkerRoute::Tool,
        BLOCK_WORLD => WorkerRoute::Block,
        RICH_WORLD => WorkerRoute::Rich,
        other => return Err(Error::UnsupportedWorld(other.to_string())),
    };

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
                match route {
                    WorkerRoute::Tool => {
                        runtime.block_on(run_tool_worker(wasm_bytes, config, event_tx_thread, meta_tx, req_rx))
                    }
                    WorkerRoute::Block => {
                        runtime.block_on(run_block_worker(wasm_bytes, config, event_tx_thread, meta_tx, req_rx))
                    }
                    WorkerRoute::Rich => {
                        runtime.block_on(run_rich_worker(wasm_bytes, config, event_tx_thread, meta_tx, req_rx))
                    }
                }
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
        has_block: matches!(route, WorkerRoute::Block | WorkerRoute::Rich),
        has_formula: matches!(route, WorkerRoute::Rich),
    })
}

/// World-routing selector used by [`spawn_worker`] to pick a worker body.
#[derive(Copy, Clone)]
enum WorkerRoute {
    /// `tool-extension` world — exports extension + tool-provider.
    Tool,
    /// `block-extension` world — adds block-provider on top.
    Block,
    /// `rich-extension` world — adds formula-provider on top of block.
    Rich,
}

/// Internal payload delivered through the meta oneshot on successful init.
struct WorkerBootstrap {
    metadata: WorkerMetadata,
}

/// The async body that runs entirely on the worker thread's current-thread
/// tokio runtime. Performs the full bring-up then enters the request loop
/// for the `tool-extension` world.
async fn run_tool_worker(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
    meta_tx: oneshot::Sender<Result<WorkerBootstrap, Error>>,
    mut req_rx: mpsc::UnboundedReceiver<Request>,
) {
    // Fused init sequence: on any failure, send Err through meta_tx and exit.
    let init = match initialize_tool(wasm_bytes, config, event_tx).await {
        Ok(ok) => ok,
        Err(err) => {
            let _ = meta_tx.send(Err(err));
            return;
        }
    };
    let InitializedTool {
        bindings,
        mut store,
        metadata,
    } = init;

    if meta_tx
        .send(Ok(WorkerBootstrap {
            metadata: metadata.into(),
        }))
        .is_err()
    {
        // The caller dropped the meta receiver — shut down cleanly.
        return;
    }

    while let Some(request) = req_rx.recv().await {
        handle_tool_request(request, &bindings, &mut store).await;
    }
}

/// The async body that runs the `block-extension` world worker.
async fn run_block_worker(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
    meta_tx: oneshot::Sender<Result<WorkerBootstrap, Error>>,
    mut req_rx: mpsc::UnboundedReceiver<Request>,
) {
    let init = match initialize_block(wasm_bytes, config, event_tx).await {
        Ok(ok) => ok,
        Err(err) => {
            let _ = meta_tx.send(Err(err));
            return;
        }
    };
    let InitializedBlock {
        bindings,
        mut store,
        metadata,
    } = init;

    if meta_tx
        .send(Ok(WorkerBootstrap {
            metadata: metadata.into(),
        }))
        .is_err()
    {
        return;
    }

    while let Some(request) = req_rx.recv().await {
        handle_block_request(request, &bindings, &mut store).await;
    }
}

/// The async body that runs the `rich-extension` world worker.
async fn run_rich_worker(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
    meta_tx: oneshot::Sender<Result<WorkerBootstrap, Error>>,
    mut req_rx: mpsc::UnboundedReceiver<Request>,
) {
    let init = match initialize_rich(wasm_bytes, config, event_tx).await {
        Ok(ok) => ok,
        Err(err) => {
            let _ = meta_tx.send(Err(err));
            return;
        }
    };
    let InitializedRich {
        bindings,
        mut store,
        metadata,
    } = init;

    if meta_tx
        .send(Ok(WorkerBootstrap {
            metadata: metadata.into(),
        }))
        .is_err()
    {
        return;
    }

    while let Some(request) = req_rx.recv().await {
        handle_rich_request(request, &bindings, &mut store).await;
    }
}

/// Output of the tool-extension initialization phase.
struct InitializedTool {
    bindings: ToolExtension,
    store: Store<State>,
    metadata: tool_bindings::exports::emporium::extensions::extension::Metadata,
}

/// Output of the block-extension initialization phase.
struct InitializedBlock {
    bindings: BlockExtension,
    store: Store<State>,
    metadata: block_bindings::exports::emporium::extensions::extension::Metadata,
}

/// Output of the rich-extension initialization phase.
struct InitializedRich {
    bindings: RichExtension,
    store: Store<State>,
    metadata: rich_bindings::exports::emporium::extensions::extension::Metadata,
}

/// Compile, instantiate, `init`, and capture metadata for tool-extension.
async fn initialize_tool(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
) -> Result<InitializedTool, Error> {
    let (engine, component, mut store) = prepare_component(&wasm_bytes, event_tx)?;

    validate_world_exports(DEFAULT_WORLD, &engine, &component)?;

    let linker = build_tool_linker(&engine)?;
    let bindings = ToolExtension::instantiate_async(&mut store, &component, &linker).await?;

    // Fetch metadata first (stateless), then call init.
    let ext_guest = bindings.emporium_extensions_extension();
    let metadata = ext_guest.call_get_metadata(&mut store).await?;
    let init_result = ext_guest.call_init(&mut store, &config).await?;
    if let Err(msg) = init_result {
        return Err(Error::ExtensionLoadError(msg));
    }

    Ok(InitializedTool {
        bindings,
        store,
        metadata,
    })
}

/// Compile, instantiate, `init`, and capture metadata for block-extension.
async fn initialize_block(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
) -> Result<InitializedBlock, Error> {
    let (engine, component, mut store) = prepare_component(&wasm_bytes, event_tx)?;

    validate_world_exports(BLOCK_WORLD, &engine, &component)?;

    let linker = build_block_linker(&engine)?;
    let bindings = BlockExtension::instantiate_async(&mut store, &component, &linker).await?;

    let ext_guest = bindings.emporium_extensions_extension();
    let metadata = ext_guest.call_get_metadata(&mut store).await?;
    let init_result = ext_guest.call_init(&mut store, &config).await?;
    if let Err(msg) = init_result {
        return Err(Error::ExtensionLoadError(msg));
    }

    Ok(InitializedBlock {
        bindings,
        store,
        metadata,
    })
}

/// Compile, instantiate, `init`, and capture metadata for rich-extension.
async fn initialize_rich(
    wasm_bytes: Vec<u8>,
    config: String,
    event_tx: broadcast::Sender<event::Event>,
) -> Result<InitializedRich, Error> {
    let (engine, component, mut store) = prepare_component(&wasm_bytes, event_tx)?;

    validate_world_exports(RICH_WORLD, &engine, &component)?;

    let linker = build_rich_linker(&engine)?;
    let bindings = RichExtension::instantiate_async(&mut store, &component, &linker).await?;

    let ext_guest = bindings.emporium_extensions_extension();
    let metadata = ext_guest.call_get_metadata(&mut store).await?;
    let init_result = ext_guest.call_init(&mut store, &config).await?;
    if let Err(msg) = init_result {
        return Err(Error::ExtensionLoadError(msg));
    }

    Ok(InitializedRich {
        bindings,
        store,
        metadata,
    })
}

/// Compile the component and build the wasmtime store with shared WASI/HTTP
/// + host-events context. Used by both world-specific initialization paths.
fn prepare_component(
    wasm_bytes: &[u8],
    event_tx: broadcast::Sender<event::Event>,
) -> Result<(Engine, Component, Store<State>), Error> {
    let mut cfg = Config::new();
    cfg.wasm_component_model(true);
    let engine = Engine::new(&cfg)?;
    let component = Component::new(&engine, wasm_bytes)?;

    let wasi = WasiCtxBuilder::new().inherit_stderr().inherit_stdout().build();
    let state = State {
        wasi,
        http: WasiHttpCtx::new(),
        table: ResourceTable::new(),
        events: event_tx,
    };
    let store = Store::new(&engine, state);
    Ok((engine, component, store))
}

/// Register WASI, WASI-HTTP, and the host-events interface for tool-extension.
fn build_tool_linker(engine: &Engine) -> Result<Linker<State>, Error> {
    let mut linker = build_base_linker(engine)?;
    ToolExtension::add_to_linker::<State, HostEventsData>(&mut linker, |s| s)?;
    Ok(linker)
}

/// Register WASI, WASI-HTTP, and the host-events interface for block-extension.
fn build_block_linker(engine: &Engine) -> Result<Linker<State>, Error> {
    let mut linker = build_base_linker(engine)?;
    BlockExtension::add_to_linker::<State, HostEventsData>(&mut linker, |s| s)?;
    Ok(linker)
}

/// Register WASI, WASI-HTTP, and the host-events interface for rich-extension.
fn build_rich_linker(engine: &Engine) -> Result<Linker<State>, Error> {
    let mut linker = build_base_linker(engine)?;
    RichExtension::add_to_linker::<State, HostEventsData>(&mut linker, |s| s)?;
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

/// Strict-match validation for a declared world. Exports must contain
/// exactly the required interface set for the world, versioned at
/// [`WIT_VERSION`], and nothing else.
///
/// | World             | Required exports                                                     |
/// |-------------------|----------------------------------------------------------------------|
/// | `tool-extension`  | `extension`, `tool-provider`                                         |
/// | `block-extension` | `extension`, `tool-provider`, `block-provider`                       |
/// | `rich-extension`  | `extension`, `tool-provider`, `block-provider`, `formula-provider`   |
fn validate_world_exports(declared: &str, engine: &Engine, component: &Component) -> Result<(), Error> {
    let required_names: &[&str] = match declared {
        DEFAULT_WORLD => &["extension", "tool-provider"],
        BLOCK_WORLD => &["extension", "tool-provider", "block-provider"],
        RICH_WORLD => &["extension", "tool-provider", "block-provider", "formula-provider"],
        other => return Err(Error::UnsupportedWorld(other.to_string())),
    };

    let required: Vec<String> = required_names
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
        return Err(Error::WorldMissingExports {
            declared: declared.to_string(),
            missing,
        });
    }

    let extra: Vec<String> = actual
        .into_iter()
        .filter(|name| !required.iter().any(|n| n == name))
        .collect();
    if !extra.is_empty() {
        return Err(Error::WorldExtraExports {
            declared: declared.to_string(),
            extra,
        });
    }

    Ok(())
}

/// Dispatch a single typed [`Request`] against the tool-extension bindings.
///
/// `Block*` variants should not arrive here — [`Extension::block`](crate::Extension::block)
/// only hands out providers when the worker is block-capable. If one does
/// arrive (caller bug), the reply surfaces [`Error::UnsupportedWorld`].
async fn handle_tool_request(request: Request, bindings: &ToolExtension, store: &mut Store<State>) {
    match request {
        Request::ListTools { reply } => {
            let outcome = tool_list_tools(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::ExecuteTool { name, params, reply } => {
            let outcome = tool_execute_tool(bindings, store, &name, &params).await;
            let _ = reply.send(outcome);
        }
        Request::View { reply } => {
            let outcome = tool_view(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::BlockTypes { reply } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
        Request::BlockCreate { reply, .. } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
        Request::BlockPlan { reply, .. } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
        Request::BlockValidate { reply, .. } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
        Request::FormulaDefs { reply } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
        Request::FormulaEvaluate { reply, .. } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(DEFAULT_WORLD.to_string())));
        }
    }
}

/// Dispatch a single typed [`Request`] against the block-extension bindings.
///
/// `Formula*` variants should not arrive here —
/// [`Extension::formula`](crate::Extension::formula) only hands out providers
/// when the worker is formula-capable. If one does arrive (caller bug), the
/// reply surfaces [`Error::UnsupportedWorld`].
async fn handle_block_request(request: Request, bindings: &BlockExtension, store: &mut Store<State>) {
    match request {
        Request::ListTools { reply } => {
            let outcome = block_list_tools(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::ExecuteTool { name, params, reply } => {
            let outcome = block_execute_tool(bindings, store, &name, &params).await;
            let _ = reply.send(outcome);
        }
        Request::View { reply } => {
            let outcome = block_view(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::BlockTypes { reply } => {
            let outcome = block_types(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::BlockCreate { kind, input, reply } => {
            let outcome = block_create(bindings, store, &kind, &input).await;
            let _ = reply.send(outcome);
        }
        Request::BlockPlan {
            kind,
            state,
            operation,
            input,
            reply,
        } => {
            let outcome = block_plan(bindings, store, &kind, &state, &operation, &input).await;
            let _ = reply.send(outcome);
        }
        Request::BlockValidate { kind, state, reply } => {
            let outcome = block_validate(bindings, store, &kind, &state).await;
            let _ = reply.send(outcome);
        }
        Request::FormulaDefs { reply } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(BLOCK_WORLD.to_string())));
        }
        Request::FormulaEvaluate { reply, .. } => {
            let _ = reply.send(Err(Error::UnsupportedWorld(BLOCK_WORLD.to_string())));
        }
    }
}

/// Dispatch a single typed [`Request`] against the rich-extension bindings.
/// Handles every variant: Tool*, View, Block*, and Formula*.
async fn handle_rich_request(request: Request, bindings: &RichExtension, store: &mut Store<State>) {
    match request {
        Request::ListTools { reply } => {
            let outcome = rich_list_tools(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::ExecuteTool { name, params, reply } => {
            let outcome = rich_execute_tool(bindings, store, &name, &params).await;
            let _ = reply.send(outcome);
        }
        Request::View { reply } => {
            let outcome = rich_view(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::BlockTypes { reply } => {
            let outcome = rich_block_types(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::BlockCreate { kind, input, reply } => {
            let outcome = rich_block_create(bindings, store, &kind, &input).await;
            let _ = reply.send(outcome);
        }
        Request::BlockPlan {
            kind,
            state,
            operation,
            input,
            reply,
        } => {
            let outcome = rich_block_plan(bindings, store, &kind, &state, &operation, &input).await;
            let _ = reply.send(outcome);
        }
        Request::BlockValidate { kind, state, reply } => {
            let outcome = rich_block_validate(bindings, store, &kind, &state).await;
            let _ = reply.send(outcome);
        }
        Request::FormulaDefs { reply } => {
            let outcome = rich_formula_defs(bindings, store).await;
            let _ = reply.send(outcome);
        }
        Request::FormulaEvaluate { name, args, reply } => {
            let outcome = rich_formula_evaluate(bindings, store, &name, &args).await;
            let _ = reply.send(outcome);
        }
    }
}

async fn tool_list_tools(bindings: &ToolExtension, store: &mut Store<State>) -> Result<Vec<tool::Info>, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_tools = guest.call_list_tools(&mut *store).await?;
    wit_tools
        .into_iter()
        .map(tool::Info::try_from)
        .collect::<Result<Vec<_>, _>>()
}

async fn tool_execute_tool(
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

async fn tool_view(bindings: &ToolExtension, store: &mut Store<State>) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_extension();
    let s = guest.call_view(&mut *store).await?;
    Ok(s)
}

async fn block_list_tools(bindings: &BlockExtension, store: &mut Store<State>) -> Result<Vec<tool::Info>, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_tools = guest.call_list_tools(&mut *store).await?;
    wit_tools
        .into_iter()
        .map(tool::Info::try_from)
        .collect::<Result<Vec<_>, _>>()
}

async fn block_execute_tool(
    bindings: &BlockExtension,
    store: &mut Store<State>,
    name: &str,
    params: &str,
) -> Result<tool::Output, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_outcome = guest.call_execute_tool(&mut *store, name, params).await?;
    let wit_output = wit_outcome.map_err(Error::ExtensionError)?;
    tool::Output::try_from(wit_output)
}

async fn block_view(bindings: &BlockExtension, store: &mut Store<State>) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_extension();
    let s = guest.call_view(&mut *store).await?;
    Ok(s)
}

async fn block_types(bindings: &BlockExtension, store: &mut Store<State>) -> Result<Vec<block::TypeInfo>, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_types = guest.call_get_block_types(&mut *store).await?;
    Ok(wit_types.into_iter().map(block::TypeInfo::from).collect())
}

async fn block_create(
    bindings: &BlockExtension,
    store: &mut Store<State>,
    kind: &str,
    input: &str,
) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_create(&mut *store, kind, input).await?;
    wit_outcome.map_err(Error::BlockOperation)
}

async fn block_plan(
    bindings: &BlockExtension,
    store: &mut Store<State>,
    kind: &str,
    state: &str,
    operation: &str,
    input: &str,
) -> Result<plan::Outcome, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_plan(&mut *store, kind, state, operation, input).await?;
    let wit_plan = wit_outcome.map_err(Error::BlockOperation)?;
    Ok(plan::Outcome::from(wit_plan))
}

async fn block_validate(
    bindings: &BlockExtension,
    store: &mut Store<State>,
    kind: &str,
    state: &str,
) -> Result<(), Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_validate(&mut *store, kind, state).await?;
    wit_outcome.map_err(Error::BlockOperation)
}

async fn rich_list_tools(bindings: &RichExtension, store: &mut Store<State>) -> Result<Vec<tool::Info>, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_tools = guest.call_list_tools(&mut *store).await?;
    wit_tools
        .into_iter()
        .map(tool::Info::try_from)
        .collect::<Result<Vec<_>, _>>()
}

async fn rich_execute_tool(
    bindings: &RichExtension,
    store: &mut Store<State>,
    name: &str,
    params: &str,
) -> Result<tool::Output, Error> {
    let guest = bindings.emporium_extensions_tool_provider();
    let wit_outcome = guest.call_execute_tool(&mut *store, name, params).await?;
    let wit_output = wit_outcome.map_err(Error::ExtensionError)?;
    tool::Output::try_from(wit_output)
}

async fn rich_view(bindings: &RichExtension, store: &mut Store<State>) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_extension();
    let s = guest.call_view(&mut *store).await?;
    Ok(s)
}

async fn rich_block_types(bindings: &RichExtension, store: &mut Store<State>) -> Result<Vec<block::TypeInfo>, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_types = guest.call_get_block_types(&mut *store).await?;
    Ok(wit_types.into_iter().map(block::TypeInfo::from).collect())
}

async fn rich_block_create(
    bindings: &RichExtension,
    store: &mut Store<State>,
    kind: &str,
    input: &str,
) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_create(&mut *store, kind, input).await?;
    wit_outcome.map_err(Error::BlockOperation)
}

async fn rich_block_plan(
    bindings: &RichExtension,
    store: &mut Store<State>,
    kind: &str,
    state: &str,
    operation: &str,
    input: &str,
) -> Result<plan::Outcome, Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_plan(&mut *store, kind, state, operation, input).await?;
    let wit_plan = wit_outcome.map_err(Error::BlockOperation)?;
    Ok(plan::Outcome::from(wit_plan))
}

async fn rich_block_validate(
    bindings: &RichExtension,
    store: &mut Store<State>,
    kind: &str,
    state: &str,
) -> Result<(), Error> {
    let guest = bindings.emporium_extensions_block_provider();
    let wit_outcome = guest.call_validate(&mut *store, kind, state).await?;
    wit_outcome.map_err(Error::BlockOperation)
}

async fn rich_formula_defs(bindings: &RichExtension, store: &mut Store<State>) -> Result<Vec<formula::Def>, Error> {
    let guest = bindings.emporium_extensions_formula_provider();
    let wit_defs = guest.call_get_formulas(&mut *store).await?;
    wit_defs
        .into_iter()
        .map(formula::Def::try_from)
        .collect::<Result<Vec<_>, _>>()
}

async fn rich_formula_evaluate(
    bindings: &RichExtension,
    store: &mut Store<State>,
    name: &str,
    args: &str,
) -> Result<String, Error> {
    let guest = bindings.emporium_extensions_formula_provider();
    let wit_outcome = guest.call_evaluate(&mut *store, name, args).await?;
    wit_outcome.map_err(Error::FormulaOperation)
}

// Conversions from wasmtime-bindgen types to canonical core types live in
// `src/convert.rs` — one set of From/TryFrom impls per bindings module.

#[cfg(test)]
mod tests {
    use super::*;
    use block_bindings::exports::emporium::extensions::block_provider as wit_block_provider;
    use tool_bindings::exports::emporium::extensions::tool_provider as wit_tool_provider;

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

    #[test]
    fn from_block_type_info_round_trip() {
        let wit = wit_block_provider::BlockTypeInfo {
            kind: "prefix-tracker".into(),
            name: "Prefix Tracker".into(),
            description: "Tracks key prefixes in the KV store".into(),
        };
        let info = block::TypeInfo::from(wit);
        assert_eq!(info.kind, "prefix-tracker");
        assert_eq!(info.name, "Prefix Tracker");
        assert_eq!(info.description, "Tracks key prefixes in the KV store");
    }

    #[test]
    fn plan_outcome_covers_all_five_variants() {
        match plan::Outcome::from(wit_block_provider::PlanOutcome::StateUpdate("s1".into())) {
            plan::Outcome::StateUpdate(s) => assert_eq!(s, "s1"),
            other => panic!("unexpected outcome: {other:?}"),
        }

        match plan::Outcome::from(wit_block_provider::PlanOutcome::Query("q1".into())) {
            plan::Outcome::Query(q) => assert_eq!(q, "q1"),
            other => panic!("unexpected outcome: {other:?}"),
        }

        let paired = wit_block_provider::PlanOutcome::StateUpdateWithQuery(("s2".into(), "q2".into()));
        match plan::Outcome::from(paired) {
            plan::Outcome::StateUpdateWithQuery(s, q) => {
                assert_eq!(s, "s2");
                assert_eq!(q, "q2");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }

        match plan::Outcome::from(wit_block_provider::PlanOutcome::Computed("c1".into())) {
            plan::Outcome::Computed(c) => assert_eq!(c, "c1"),
            other => panic!("unexpected outcome: {other:?}"),
        }

        let paired = wit_block_provider::PlanOutcome::StateUpdateWithComputed(("s3".into(), "c2".into()));
        match plan::Outcome::from(paired) {
            plan::Outcome::StateUpdateWithComputed(s, c) => {
                assert_eq!(s, "s3");
                assert_eq!(c, "c2");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn from_formula_def_round_trip() {
        use rich_bindings::exports::emporium::extensions::formula_provider as wit_formula_provider_rich;

        let schema_json = r#"{"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}"#;
        let wit = wit_formula_provider_rich::FormulaDef {
            name: "KV_GET".into(),
            description: "Read a value by key".into(),
            schema: schema_json.into(),
        };

        let def = formula::Def::try_from(wit).unwrap();
        assert_eq!(def.name, "KV_GET");
        assert_eq!(def.description, "Read a value by key");
        assert_eq!(
            def.schema,
            serde_json::from_str::<serde_json::Value>(schema_json).unwrap()
        );
    }

    #[test]
    fn from_formula_def_empty_schema_yields_null() {
        use rich_bindings::exports::emporium::extensions::formula_provider as wit_formula_provider_rich;

        let wit = wit_formula_provider_rich::FormulaDef {
            name: "KV_COUNT".into(),
            description: "".into(),
            schema: "".into(),
        };
        let def = formula::Def::try_from(wit).unwrap();
        assert_eq!(def.name, "KV_COUNT");
        assert_eq!(def.schema, serde_json::Value::Null);
    }

    #[test]
    fn from_block_type_info_round_trip_rich() {
        use rich_bindings::exports::emporium::extensions::block_provider as wit_block_provider_rich;

        let wit = wit_block_provider_rich::BlockTypeInfo {
            kind: "prefix-tracker".into(),
            name: "Prefix Tracker".into(),
            description: "Tracks key prefixes in the KV store".into(),
        };
        let info = block::TypeInfo::from(wit);
        assert_eq!(info.kind, "prefix-tracker");
        assert_eq!(info.name, "Prefix Tracker");
        assert_eq!(info.description, "Tracks key prefixes in the KV store");
    }
}
