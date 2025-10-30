//! WASM extension support
use std::pin::Pin;

use futures::StreamExt;
use futures::channel::mpsc;
use sipper::{Sipper, sipper};
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use crate::Error;
use crate::data::{Command, Event, Id};

/// Public type aliases for easier consumer access
pub type Sender = futures::channel::mpsc::UnboundedSender<Command>;
pub type Receiver = futures::channel::mpsc::UnboundedReceiver<Command>;

pub(crate) struct State {
    table: wasmtime_wasi::ResourceTable,
    wasi: wasmtime_wasi::WasiCtx,
    http: wasmtime_wasi_http::types::WasiHttpCtx,
}

// TODO: Arc not Clone?
#[derive(Clone)]
pub struct Extension {
    id: Id,
    wasm_bytes: Vec<u8>,
    config: String,
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

// Implement WasiHttpView for State to enable HTTP support
impl wasmtime_wasi_http::types::WasiHttpView for State {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut wasmtime_wasi_http::types::WasiHttpCtx {
        &mut self.http
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
    /// Set the configuration JSON for the extension
    pub fn with_config(mut self, config: String) -> Self {
        self.config = config;
        self
    }

    /// Convert the extension into a sipper that emits responses.
    /// The sipper will first emit a Connected response with a message sender.
    pub fn into_sipper(self) -> impl Sipper<(), Event> {
        let (msg_tx, mut msg_rx): (Sender, Receiver) = mpsc::unbounded();

        sipper(move |mut output| async move {
            let mut config = wasmtime::Config::new();
            config.async_support(true);
            let engine = Engine::new(&config).unwrap();
            let component = Component::from_binary(&engine, &self.wasm_bytes).unwrap();

            // Set up the WASM instance
            let mut linker = Linker::new(&engine);
            bindings::ExtensionWorld::add_to_linker(&mut linker, |state: &mut State| state).unwrap();

            // Add WASI support
            wasmtime_wasi::add_to_linker_async(&mut linker).unwrap();

            // Add WASI HTTP support
            wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker).unwrap();

            // Create WASI context
            let wasi = wasmtime_wasi::WasiCtxBuilder::new().inherit_stdio().build();

            let mut store = Store::new(&engine, State {
                table: wasmtime_wasi::ResourceTable::new(),
                wasi,
                http: wasmtime_wasi_http::types::WasiHttpCtx::new(),
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
                .send(Event::Core(emporium_core::Response::Metadata {
                    id: metadata.id,
                    name: metadata.name,
                    version: metadata.version,
                    description: metadata.description,
                }))
                .await;

            // Create an extension instance
            let instance = bindings.emporium_extensions_extension().instance();

            // Create instance resource with config
            let instance_resource = instance.call_new(&mut store, &self.config).await.unwrap();

            // Send the Connected response with the message sender
            output.send(Event::Connected(msg_tx.clone())).await;

            // Process messages
            while let Some(cmd) = msg_rx.next().await {
                // Extract correlation_id for error handling
                let correlation_id = cmd.correlation_id().cloned();

                // Serialize the command to JSON
                let cmd_json = match serde_json::to_string(&cmd) {
                    Ok(json) => json,
                    Err(e) => {
                        output
                            .send(Event::Core(emporium_core::Response::Error {
                                message: format!("Failed to serialize command: {}", e),
                                correlation_id,
                            }))
                            .await;
                        continue;
                    }
                };

                eprintln!("Processing command: {}", cmd_json);

                // Pass the JSON string to the extension
                match instance.call_update(&mut store, instance_resource, &cmd_json).await {
                    Ok(Ok(response_json)) => {
                        // Try to deserialize the response as core Response enum
                        match serde_json::from_str::<emporium_core::Response>(&response_json) {
                            Ok(core_response) => {
                                output.send(Event::Core(core_response)).await;
                            }
                            Err(e) => {
                                output
                                    .send(Event::Core(emporium_core::Response::Error {
                                        message: format!("Failed to deserialize response: {}", e),
                                        correlation_id,
                                    }))
                                    .await;
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        // Extension returned an error
                        output
                            .send(Event::Core(emporium_core::Response::Error {
                                message: error,
                                correlation_id,
                            }))
                            .await;
                    }
                    Err(e) => {
                        // WASM runtime error
                        output
                            .send(Event::Core(emporium_core::Response::Error {
                                message: format!("Runtime error: {}", e),
                                correlation_id,
                            }))
                            .await;
                    }
                }
            }

            eprintln!("Extension {} message loop ended", self.id);
        })
    }
}

/// Load an extension by ID
pub async fn load(id: Id, config: String, path: std::path::PathBuf) -> Result<Extension, Error> {
    let wasm_path = if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
        // Path is already pointing to a WASM file
        path.to_path_buf()
    } else {
        // Path is a directory, look for extension.wasm inside it
        path.join("extension.wasm")
    };

    if wasm_path.exists() {
        let wasm_bytes = std::fs::read(wasm_path)?;

        Ok(Extension { id, wasm_bytes, config })
    } else {
        Err(Error::ExtensionNotFound(wasm_path.display().to_string()))
    }
}

impl std::fmt::Debug for Extension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmExtension").field("id", &self.id).finish()
    }
}
