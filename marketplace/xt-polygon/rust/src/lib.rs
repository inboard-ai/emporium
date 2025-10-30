//! Polygon.io market data extension using WASI HTTP and the polygon crate
#![allow(unsafe_op_in_unsafe_fn)]

mod command;

wit_bindgen::generate!({
    path: "./wit",
    world: "extension-world",
    generate_all,
});

use exports::emporium::extensions::extension::{Guest, Instance, Metadata};
use serde::Deserialize;
use serde_json::json;
use std::cell::RefCell;

use polygon;
use polygon::{Polygon, Request};

// WASI HTTP types
use wasi::http::outgoing_handler;
use wasi::http::types::*;

// WASI HTTP client that implements polygon::Request trait
struct WasiHttpClient;

pub struct HttpResponse {
    status: u16,
    body: String,
    request_id: Option<String>,
}

impl polygon::Response for HttpResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn body(&self) -> &str {
        &self.body
    }

    fn request_id(&self) -> &Option<String> {
        &self.request_id
    }
}

impl Request for WasiHttpClient {
    type Response = HttpResponse;

    fn new() -> Self {
        WasiHttpClient
    }

    async fn get(&self, url: &str) -> Result<Self::Response, polygon::Error> {
        let url = url.to_string();
        Self::make_http_request(&url, Method::Get, None).await
    }

    async fn post(&self, url: &str, body: &str) -> Result<Self::Response, polygon::Error> {
        let url = url.to_string();
        let body = body.to_string();
        Self::make_http_request(&url, Method::Post, Some(&body)).await
    }
}

// TODO: Move this out of the struct, into a separate crate-level `http.rs` module.
impl WasiHttpClient {
    async fn make_http_request(url: &str, method: Method, body: Option<&str>) -> Result<HttpResponse, polygon::Error> {
        log("debug", &format!("Making HTTP request to {url}"));
        // Parse URL to extract components (https://api.polygon.io/path)
        let url_parts: Vec<&str> = url.splitn(3, '/').collect();
        if url_parts.len() < 3 {
            return Err(polygon::Error::Custom("Invalid URL".to_string()));
        }

        let authority = url_parts[2].split('/').next().unwrap_or("");
        let path_and_query = url.split(authority).nth(1).unwrap_or("");

        // Create outgoing request
        let req = OutgoingRequest::new(Fields::new());
        req.set_scheme(Some(&Scheme::Https))
            .map_err(|_| polygon::Error::Custom("Failed to set scheme".to_string()))?;
        req.set_authority(Some(authority))
            .map_err(|_| polygon::Error::Custom("Failed to set authority".to_string()))?;
        req.set_path_with_query(Some(path_and_query))
            .map_err(|_| polygon::Error::Custom("Failed to set path".to_string()))?;
        req.set_method(&method)
            .map_err(|_| polygon::Error::Custom("Failed to set method".to_string()))?;

        // Add body for POST requests
        if let Some(body_str) = body {
            let outgoing_body = req
                .body()
                .map_err(|_| polygon::Error::Custom("Failed to get request body".to_string()))?;
            let body_stream = outgoing_body
                .write()
                .map_err(|_| polygon::Error::Custom("Failed to get body stream".to_string()))?;
            body_stream
                .blocking_write_and_flush(body_str.as_bytes())
                .map_err(|_| polygon::Error::Custom("Failed to write body".to_string()))?;
            drop(body_stream);
            OutgoingBody::finish(outgoing_body, None).ok();
        }

        // Send request
        let future_response = outgoing_handler::handle(req, None)
            .map_err(|_| polygon::Error::Custom("Failed to send request".to_string()))?;

        // Wait for response using WASI polling
        let pollable = future_response.subscribe();
        pollable.block();

        let incoming_response = future_response
            .get()
            .ok_or_else(|| polygon::Error::Custom("Response not ready".to_string()))?
            .map_err(|_| polygon::Error::Custom("Request failed".to_string()))?
            .map_err(|_| polygon::Error::Custom("HTTP error".to_string()))?;

        let status = incoming_response.status();

        // Try to extract request ID from headers
        let headers = incoming_response.headers();
        let request_id = headers
            .get(&"x-request-id".to_string())
            .first()
            .and_then(|value| String::from_utf8(value.clone()).ok())
            .or_else(|| {
                // Polygon.io might use a different header name
                headers
                    .get(&"x-trace-id".to_string())
                    .first()
                    .and_then(|value| String::from_utf8(value.clone()).ok())
            });

        // Read response body
        let body_stream = incoming_response
            .consume()
            .map_err(|_| polygon::Error::Custom("Failed to get response body".to_string()))?;

        let mut body_bytes = Vec::new();
        let input_stream = body_stream
            .stream()
            .map_err(|_| polygon::Error::Custom("Failed to get input stream".to_string()))?;

        loop {
            match input_stream.blocking_read(4096) {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        break;
                    }
                    body_bytes.extend_from_slice(&chunk);
                }
                Err(_) => break,
            }
        }

        let body =
            String::from_utf8(body_bytes).map_err(|e| polygon::Error::Custom(format!("Invalid UTF-8: {}", e)))?;

        Ok(HttpResponse {
            status: status as u16,
            body,
            request_id,
        })
    }
}

// Main extension component wrapper
struct Wrapper;

impl Guest for Wrapper {
    type Instance = PolygonExtension;

    fn get_metadata() -> Metadata {
        Metadata {
            id: "polygon".to_string(),
            name: "Polygon.io Market Data".to_string(),
            version: "0.1.0".to_string(),
            description: "Access real-time and historical market data from Polygon.io".to_string(),
        }
    }
}

// Wrapper for the Polygon extension
struct PolygonExtension(RefCell<Internal>);

// Internal state
struct Internal(Polygon<WasiHttpClient>);

impl Internal {
    fn init(config: &str) -> Self {
        // Parse config to extract api_key
        #[derive(Deserialize)]
        struct Config {
            api_key: String,
        }

        let parsed_config: Config = serde_json::from_str(config).expect("Failed to parse config JSON");

        // Create Polygon client with WASI HTTP implementation
        let client = Polygon::default()
            .with_client(WasiHttpClient)
            .with_key(&parsed_config.api_key);

        log(
            "info",
            &format!(
                "Initialized Polygon client with WASI HTTP using API key: {}...",
                &parsed_config.api_key[..3.min(parsed_config.api_key.len())]
            ),
        );
        Self(client)
    }

    async fn handle_command(&self, command: String) -> Result<String, String> {
        // Parse the command
        let cmd = serde_json::from_str(&command).map_err(|e| format!("Invalid command: {}", e))?;

        // Handle command and get response
        let response = command::respond(&self.0, cmd).await;

        // Serialize the response
        serde_json::to_string(&response).map_err(|e| format!("Failed to serialize response: {}", e))
    }
}

impl exports::emporium::extensions::extension::GuestInstance for PolygonExtension {
    fn new(config: String) -> Instance {
        log("info", "Creating new Polygon extension instance");
        let extension = PolygonExtension(RefCell::new(Internal::init(&config)));

        Instance::new(extension)
    }

    fn update(&self, command: String) -> Result<String, String> {
        // Just pass through to the async handler
        futures::executor::block_on(self.0.borrow().handle_command(command))
    }

    fn view(&self) -> String {
        // let internal = self.0.borrow();
        json!({
            "type": "polygon_extension_info",
        })
        .to_string()
    }
}

export!(Wrapper);
