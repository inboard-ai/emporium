//! High-level typed extension API.
//!
//! A loaded [`Extension`] is `Send + Sync + Clone`: clones share the same
//! worker thread via a cheap `mpsc` sender clone, and the thread exits when
//! the last clone drops. All typed methods are `async` and round-trip a
//! request through the worker.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use emporium_core::{event, tool};
use flate2::read::GzDecoder;
use tar::Archive;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::host_data::DataRegistry;
use crate::manifest::{Manifest, RawManifest};
use crate::wasm::{self, Request};
use crate::{Error, ManifestTool, block, formula};

/// A loaded extension, ready to receive typed calls.
///
/// Cloning an `Extension` is cheap: the clone shares the same worker thread
/// and broadcast subscriber side. The worker thread exits only once the
/// final clone drops.
#[derive(Clone)]
pub struct Extension {
    manifest: Arc<Manifest>,
    config: serde_json::Value,
    metadata: CachedMetadata,
    requests: mpsc::UnboundedSender<Request>,
    events: broadcast::Sender<event::Event>,
    /// True when the extension exports the `block-provider` interface.
    /// Gates [`Extension::block`](Self::block).
    has_block: bool,
    /// True when the extension exports the `formula-provider` interface.
    /// Gates [`Extension::formula`](Self::formula).
    has_formula: bool,
    /// True when the extension was loaded against a world that imports
    /// `host-data` (currently only `full-extension`).
    has_host_data: bool,
}

/// Metadata captured from `extension::get-metadata` during init, kept on the
/// Extension for cheap synchronous accessors.
#[derive(Debug, Clone)]
struct CachedMetadata {
    id: String,
    name: String,
    version: String,
    description: String,
}

impl Extension {
    /// Load an extension from a `.empkg` package file on disk. Uses an
    /// empty host-data registry — callers that need to expose resources
    /// to the extension should use [`Self::load_with_data`].
    pub async fn load(path: impl AsRef<Path>, config: serde_json::Value) -> Result<Self, Error> {
        Self::load_with_data(path, config, DataRegistry::default()).await
    }

    /// Load an extension from pre-read package bytes. Uses an empty
    /// host-data registry — see [`Self::from_bytes_with_data`] for the
    /// host-data-enabled form.
    pub async fn from_bytes(bytes: &[u8], config: serde_json::Value) -> Result<Self, Error> {
        Self::from_bytes_with_data(bytes, config, DataRegistry::default()).await
    }

    /// Load an extension from a `.empkg` file, providing a [`DataRegistry`]
    /// that an extension loaded against `full-extension` can stream from.
    pub async fn load_with_data(
        path: impl AsRef<Path>,
        config: serde_json::Value,
        data: DataRegistry,
    ) -> Result<Self, Error> {
        let bytes = tokio::fs::read(path.as_ref()).await?;
        Self::from_bytes_with_data(&bytes, config, data).await
    }

    /// Load an extension from pre-read package bytes, providing a
    /// [`DataRegistry`] that an extension loaded against `full-extension`
    /// can stream from.
    pub async fn from_bytes_with_data(
        bytes: &[u8],
        config: serde_json::Value,
        data: DataRegistry,
    ) -> Result<Self, Error> {
        let (manifest, wasm_bytes) = extract_package(bytes)?;
        let manifest = Arc::new(manifest);
        let config_str = serde_json::to_string(&config)?;

        let worker = wasm::spawn_worker(manifest.clone(), wasm_bytes, config_str, data).await?;

        let metadata = CachedMetadata {
            id: worker.metadata.id,
            name: worker.metadata.name,
            version: worker.metadata.version,
            description: worker.metadata.description,
        };

        Ok(Extension {
            manifest,
            config,
            metadata,
            requests: worker.requests,
            events: worker.events,
            has_block: worker.has_block,
            has_formula: worker.has_formula,
            has_host_data: worker.has_host_data,
        })
    }

    /// Parsed manifest backing this extension.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Extension identifier (`get-metadata().id`).
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    /// Human-readable extension name.
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    /// Extension version as reported by the component itself.
    pub fn version(&self) -> &str {
        &self.metadata.version
    }

    /// Description returned by `get-metadata`.
    pub fn description(&self) -> &str {
        &self.metadata.description
    }

    /// Configuration the extension was initialized with.
    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }

    /// Tools advertised in the manifest (build-time cache). Runtime
    /// [`list_tools`](Self::list_tools) is authoritative.
    pub fn manifest_tools(&self) -> &[ManifestTool] {
        &self.manifest.tools
    }

    /// Ask the extension for its current list of tools.
    pub async fn list_tools(&self) -> Result<Vec<tool::Info>, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::ListTools { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Execute a named tool with JSON parameters.
    pub async fn execute_tool(&self, name: &str, params: serde_json::Value) -> Result<tool::Output, Error> {
        let params_str = serde_json::to_string(&params)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::ExecuteTool {
                name: name.to_string(),
                params: params_str,
                reply: reply_tx,
            })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Render the extension's current view (typically used by the REPL).
    pub async fn view(&self) -> Result<String, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::View { reply: reply_tx })
            .map_err(|_| Error::ExtensionCrashed)?;
        reply_rx.await.map_err(|_| Error::ExtensionCrashed)?
    }

    /// Subscribe to the host-events broadcast stream. Each call yields a
    /// fresh receiver; slow consumers observe `RecvError::Lagged`.
    pub fn events(&self) -> broadcast::Receiver<event::Event> {
        self.events.subscribe()
    }

    /// Return a [`block::Provider`] when the extension was loaded against a
    /// world that exports `block-provider`. Returns `None` for
    /// `tool-extension`-only extensions. Each call yields a fresh provider
    /// sharing the same underlying worker channel — cheap to clone.
    pub fn block(&self) -> Option<block::Provider> {
        self.has_block.then(|| block::Provider::new(self.requests.clone()))
    }

    /// Return a [`formula::Provider`] when the extension was loaded against a
    /// world that exports `formula-provider`. Returns `None` for worlds that
    /// do not (tool-extension, block-extension). Each call yields a fresh
    /// provider sharing the same underlying worker channel — cheap to clone.
    pub fn formula(&self) -> Option<formula::Provider> {
        self.has_formula.then(|| formula::Provider::new(self.requests.clone()))
    }

    /// True when the extension was loaded against `full-extension` and can
    /// open host-data cursors via its import of the `host-data` interface.
    pub fn has_host_data(&self) -> bool {
        self.has_host_data
    }
}

// Compile-time guarantee: `Extension` must stay `Send + Sync` so host apps
// can freely share clones across tasks and threads.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Extension>();
};

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extension")
            .field("id", &self.metadata.id)
            .field("name", &self.metadata.name)
            .field("version", &self.metadata.version)
            .field("has_block", &self.has_block)
            .field("has_formula", &self.has_formula)
            .field("has_host_data", &self.has_host_data)
            .finish()
    }
}

/// Extract the manifest and WASM bytes from a gzipped-tar `.empkg` archive.
fn extract_package(bytes: &[u8]) -> Result<(Manifest, Vec<u8>), Error> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);

    let mut manifest_content: Option<String> = None;
    let mut wasm_bytes: Option<Vec<u8>> = None;

    for entry in archive
        .entries()
        .map_err(|e| Error::Custom(format!("Invalid package: {e}")))?
    {
        let mut entry = entry.map_err(|e| Error::Custom(format!("Invalid package entry: {e}")))?;
        let path = entry.path().map_err(|e| Error::Custom(format!("Invalid path: {e}")))?;
        let path_str = path.to_string_lossy();

        if path_str.ends_with("manifest.toml") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| Error::Custom(format!("Failed to read manifest: {e}")))?;
            manifest_content = Some(content);
        } else if path_str.ends_with(".wasm") {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Custom(format!("Failed to read WASM: {e}")))?;
            wasm_bytes = Some(bytes);
        }
    }

    let manifest_content = manifest_content.ok_or_else(|| Error::Custom("Package missing manifest.toml".into()))?;
    let wasm_bytes = wasm_bytes.ok_or_else(|| Error::Custom("Package missing WASM file".into()))?;

    let raw: RawManifest =
        toml::from_str(&manifest_content).map_err(|e| Error::Custom(format!("Invalid manifest: {e}")))?;
    let manifest = raw.into_manifest()?;

    Ok((manifest, wasm_bytes))
}
