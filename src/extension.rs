//! High-level extension API.
//!
//! This module provides a clean abstraction for loading and communicating with extensions.
//! All WASM/runtime details are hidden from consumers.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use futures::channel::mpsc;
use futures::{Stream, StreamExt};
use tar::Archive;

use crate::data::{Command, Event};
use crate::manifest::{Manifest, RawManifest};
use crate::{Error, wasm};

/// A loaded extension ready to communicate with.
///
/// All WASM/runtime details are hidden. Use [`Extension::load`] to load from
/// a `.empkg` package file, or [`Extension::from_bytes`] to load from bytes.
pub struct Extension {
    manifest: Manifest,
    config: serde_json::Value,
    sender: Option<mpsc::UnboundedSender<Command>>,
    events: Option<futures::stream::BoxStream<'static, Event>>,
}

impl Extension {
    /// Load an extension from a `.empkg` package file.
    ///
    /// The `config` parameter provides settings for the extension (e.g., API keys).
    /// The schema for required settings is available via [`Extension::manifest`].
    pub async fn load(path: impl AsRef<Path>, config: serde_json::Value) -> Result<Self, Error> {
        let bytes = tokio::fs::read(path.as_ref()).await?;
        Self::from_bytes(&bytes, config).await
    }

    /// Load an extension from package bytes (e.g., downloaded from a store).
    pub async fn from_bytes(bytes: &[u8], config: serde_json::Value) -> Result<Self, Error> {
        // Extract manifest and WASM from the package
        let (manifest, wasm_bytes) = extract_package(bytes)?;

        // Create the WASM extension (internal)
        let config_str = serde_json::to_string(&config)?;
        let wasm_ext = wasm::Extension::new(manifest.id.clone(), wasm_bytes, config_str);

        // Convert to sipper and set up communication
        let mut sipper = Box::pin(wasm_ext.into_sipper());

        // Wait for the Connected event to get the sender
        let sender;
        let mut pending_events = Vec::new();

        loop {
            match sipper.next().await {
                Some(Event::Connected(tx)) => {
                    sender = tx;
                    break;
                }
                Some(event) => {
                    // Buffer any events before Connected
                    pending_events.push(event);
                }
                None => {
                    return Err(Error::Custom("Extension closed before connecting".into()));
                }
            }
        }

        // Create event stream that includes any buffered events
        let events: futures::stream::BoxStream<'static, Event> = if pending_events.is_empty() {
            sipper.boxed()
        } else {
            let buffered = futures::stream::iter(pending_events);
            buffered.chain(sipper).boxed()
        };

        Ok(Extension {
            manifest,
            config,
            sender: Some(sender),
            events: Some(events),
        })
    }

    /// Get the extension manifest (id, name, version, config schema, etc.).
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Get the extension ID.
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Get the extension version.
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// Get current settings/config.
    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }

    /// Update settings/config.
    ///
    /// Note: This updates the local config state. The extension will receive
    /// the new config on the next command that uses it.
    pub fn set_config(&mut self, config: serde_json::Value) {
        self.config = config;
    }

    /// Get a clone of the command sender, if connected.
    pub fn sender(&self) -> Option<mpsc::UnboundedSender<Command>> {
        self.sender.clone()
    }

    /// Send a command to the extension.
    pub fn send(&self, command: Command) -> Result<(), Error> {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| Error::Custom("Extension not connected".into()))?;

        sender
            .unbounded_send(command)
            .map_err(|e| Error::Custom(format!("Failed to send command: {}", e)))
    }

    /// Get a stream of events from the extension.
    ///
    /// This method takes ownership of the event stream. It can only be called once.
    pub fn events(&mut self) -> Option<impl Stream<Item = Event> + 'static> {
        self.events.take()
    }
}

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extension")
            .field("id", &self.manifest.id)
            .field("name", &self.manifest.name)
            .field("version", &self.manifest.version)
            .finish()
    }
}

/// Extract manifest and WASM bytes from a .empkg package.
fn extract_package(bytes: &[u8]) -> Result<(Manifest, Vec<u8>), Error> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);

    let mut manifest_content: Option<String> = None;
    let mut wasm_bytes: Option<Vec<u8>> = None;

    for entry in archive
        .entries()
        .map_err(|e| Error::Custom(format!("Invalid package: {}", e)))?
    {
        let mut entry = entry.map_err(|e| Error::Custom(format!("Invalid package entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| Error::Custom(format!("Invalid path: {}", e)))?;
        let path_str = path.to_string_lossy();

        if path_str.ends_with("manifest.toml") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| Error::Custom(format!("Failed to read manifest: {}", e)))?;
            manifest_content = Some(content);
        } else if path_str.ends_with(".wasm") {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Custom(format!("Failed to read WASM: {}", e)))?;
            wasm_bytes = Some(bytes);
        }
    }

    let manifest_content = manifest_content.ok_or_else(|| Error::Custom("Package missing manifest.toml".into()))?;
    let wasm_bytes = wasm_bytes.ok_or_else(|| Error::Custom("Package missing WASM file".into()))?;

    // Parse the manifest
    let raw: RawManifest =
        toml::from_str(&manifest_content).map_err(|e| Error::Custom(format!("Invalid manifest: {}", e)))?;
    let manifest = raw.into_manifest()?;

    Ok((manifest, wasm_bytes))
}
