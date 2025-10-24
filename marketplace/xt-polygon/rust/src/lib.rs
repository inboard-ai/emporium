//! Polygon.io market data extension using WASI HTTP and the polygon crate
#![allow(unsafe_op_in_unsafe_fn)]

wit_bindgen::generate!({
    path: "./wit",
    world: "extension-world",
    generate_all,
});

use exports::emporium::extensions::extension::{Guest, Instance, Metadata};
use serde::Deserialize;
use serde_json::json;
use std::cell::RefCell;
use std::future::Future;

use polygon;
use polygon::{Polygon, Request, Response};

// Import WASI HTTP types
use wasi::http::outgoing_handler;
use wasi::http::types::*;

// WASI HTTP client that implements polygon::Request trait
struct WasiHttpClient;

pub struct HttpResponse {
    status: u16,
    body: String,
}

impl Response for HttpResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn body(self) -> String {
        self.body
    }
}

impl Request for WasiHttpClient {
    type Response = HttpResponse;

    fn new() -> Self {
        WasiHttpClient
    }

    fn get(&self, url: &str) -> impl Future<Output = Result<Self::Response, polygon::Error>> + Send {
        let url = url.to_string();
        async move { Self::make_http_request(&url, Method::Get, None).await }
    }

    fn post(&self, url: &str, body: &str) -> impl Future<Output = Result<Self::Response, polygon::Error>> + Send {
        let url = url.to_string();
        let body = body.to_string();
        async move { Self::make_http_request(&url, Method::Post, Some(&body)).await }
    }
}

impl WasiHttpClient {
    async fn make_http_request(url: &str, method: Method, body: Option<&str>) -> Result<HttpResponse, polygon::Error> {
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
        })
    }
}

// Main component
struct Component;

// Commands that can be sent to the polygon extension
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum Command {
    /// Get related tickers
    RelatedTickers { ticker: String },
    /// List all tickers with optional params
    ListTickers {
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        exchange: Option<String>,
    },
    /// Get ticker details
    TickerDetails { ticker: String },
}

// Wrapper for the Polygon extension
struct PolygonExtension(RefCell<Internal>);

// Internal state
struct Internal(Polygon<WasiHttpClient>);

impl Internal {
    fn init(key: &str) -> Self {
        // Create Polygon client with WASI HTTP implementation
        let client = Polygon::with_client(WasiHttpClient).with_key(key);

        log("info", "Initialized Polygon client with WASI HTTP");
        Self(client)
    }

    async fn update(&self, cmd: Command) -> Result<String, String> {
        use polygon::query::Execute;
        use polygon::rest::raw;

        match cmd {
            Command::RelatedTickers { ticker } => {
                // Use polygon's raw JSON API for related tickers
                raw::tickers::related(&self.0, &ticker)
                    .get()
                    .await
                    .map_err(|e| format!("Failed to get related tickers: {:?}", e))
            }
            Command::ListTickers { limit, exchange } => {
                // Build query for listing tickers
                let mut query = raw::tickers::all(&self.0);

                if let Some(limit) = limit {
                    query = query.param("limit", limit);
                }
                if let Some(exchange) = exchange {
                    query = query.param("exchange", exchange);
                }

                query
                    .get()
                    .await
                    .map_err(|e| format!("Failed to list tickers: {:?}", e))
            }
            Command::TickerDetails { ticker } => {
                // Use polygon's raw JSON API for ticker details
                raw::tickers::details(&self.0, &ticker)
                    .get()
                    .await
                    .map_err(|e| format!("Failed to get ticker details: {:?}", e))
            }
        }
    }
}

impl Guest for Component {
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

// Implement the resource instance methods
impl exports::emporium::extensions::extension::GuestInstance for PolygonExtension {
    fn new(config: String) -> Instance {
        log("info", "Creating new Polygon extension instance");
        let extension = PolygonExtension(RefCell::new(Internal::init(&config)));

        Instance::new(extension)
    }

    fn update(&self, command: String) -> Result<String, String> {
        // Parse command
        let cmd: Command = serde_json::from_str(&command)
            .map_err(|e| format!("Invalid command: {}", e))?;

        log("debug", &format!("Handling command: {:?}", cmd));

        // Handle the command - block on the async call
        // The host handles this asynchronously even though we block here
        futures::executor::block_on(self.0.borrow().update(cmd))
    }

    fn view(&self) -> String {
        // let internal = self.0.borrow();
        json!({
            "type": "polygon_extension_info",
            // "initialized": format!("{:?}", internal.client),
        })
        .to_string()
    }
}

export!(Component);
