//! WASM extension support
use std::pin::Pin;

use futures::StreamExt;
use futures::channel::mpsc;
use sipper::{Sipper, sipper};
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::Error;
use crate::data::{Id, Message, Response};

pub(crate) struct State {
    table: wasmtime_wasi::ResourceTable,
    wasi: wasmtime_wasi::WasiCtx,
}

// TODO: Arc not Clone?
#[derive(Clone)]
pub struct Extension {
    id: Id,
    wasm_bytes: Vec<u8>,
}

pub(crate) mod bindings {
    // Generate host-side bindings from WIT
    wasmtime::component::bindgen!({
        path: "./wit",
        world: "extension-world",
        async: true,
    });
}

// Implement the types::Host trait (empty trait required by add_to_linker)
impl bindings::emporium::extensions::types::Host for State {}

// Implement WasiView for State so WASI can access the context
impl wasmtime_wasi::WasiView for State {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut wasmtime_wasi::WasiCtx {
        &mut self.wasi
    }
}

// Implement the log function that extensions can call
impl bindings::ExtensionWorldImports for State {
    fn log<'a, 'b>(
        &'a mut self,
        level: String,
        message: String,
    ) -> Pin<Box<dyn futures::Future<Output = ()> + Send + 'b>>
    where
        'a: 'b,
        Self: 'b,
    {
        Box::pin(async move {
            eprintln!("[{}] {}", level, message);
        })
    }
}

impl Extension {
    /// Convert the extension into a sipper that emits responses.
    /// Returns (sipper, message_sender) where you send messages to the extension via the sender.
    pub fn into_sipper(self) -> (impl Sipper<(), Response>, mpsc::UnboundedSender<Message>) {
        let (msg_tx, mut msg_rx): (mpsc::UnboundedSender<Message>, mpsc::UnboundedReceiver<Message>) =
            mpsc::unbounded();
        let sipper = sipper(move |mut output| async move {
            let mut config = wasmtime::Config::new();
            config.async_support(true);
            let engine = Engine::new(&config).unwrap();
            let component = Component::from_binary(&engine, &self.wasm_bytes).unwrap();

            // Set up the WASM instance
            let mut linker = Linker::new(&engine);
            bindings::ExtensionWorld::add_to_linker(&mut linker, |state: &mut State| state).unwrap();

            // Add WASI support
            wasmtime_wasi::add_to_linker_async(&mut linker).unwrap();

            // Create WASI context
            let wasi = wasmtime_wasi::WasiCtxBuilder::new().inherit_stdio().build();

            let mut store = Store::new(&engine, State {
                table: wasmtime_wasi::ResourceTable::new(),
                wasi,
            });

            let bindings = bindings::ExtensionWorld::instantiate_async(&mut store, &component, &linker)
                .await
                .unwrap();

            // Get metadata
            let metadata = bindings
                .emporium_extensions_extension()
                .call_get_metadata(&mut store)
                .await
                .unwrap();

            output
                .send(Response(
                    serde_json::json!({
                        "type": "metadata",
                        "id": metadata.id,
                        "name": metadata.name,
                        "version": metadata.version,
                        "description": metadata.description
                    })
                    .to_string(),
                ))
                .await;

            // Create an extension instance
            let instance = bindings.emporium_extensions_extension().instance();

            // Create instance resource
            let instance_resource = instance.call_new(&mut store, &"{}").await.unwrap();

            output
                .send(Response(
                    serde_json::json!({
                        "type": "ready",
                        "extension_id": self.id
                    })
                    .to_string(),
                ))
                .await;

            // Process messages
            while let Some(msg) = msg_rx.next().await {
                // Pass the message string directly to the extension
                match instance.call_update(&mut store, instance_resource, &msg.0).await {
                    Ok(Ok(response)) => {
                        // Extension returned success - pass the string through
                        output.send(Response(response)).await;
                    }
                    Ok(Err(error)) => {
                        // Extension returned an error - wrap it in error JSON
                        output
                            .send(Response(
                                serde_json::json!({
                                    "error": error
                                })
                                .to_string(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        // WASM runtime error
                        output
                            .send(Response(
                                serde_json::json!({
                                    "error": format!("Runtime error: {}", e)
                                })
                                .to_string(),
                            ))
                            .await;
                    }
                }
            }
        });

        (sipper, msg_tx)
    }
}

/// Load an extension by ID
pub async fn load(id: Id, path: std::path::PathBuf) -> Result<Extension, Error> {
    let wasm_path = if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        // Path is already pointing to a WASM file
        path.to_path_buf()
    } else {
        // Path is a directory, look for extension.wasm inside it
        path.join("extension.wasm")
    };

    if wasm_path.exists() {
        let wasm_bytes = std::fs::read(wasm_path)?;

        Ok(Extension { id, wasm_bytes })
    } else {
        Err(Error::ExtensionNotFound(wasm_path.display().to_string()))
    }
}

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmExtension").field("id", &self.id).finish()
    }
}
