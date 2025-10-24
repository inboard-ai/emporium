//! Simple KV extension
#![allow(unsafe_op_in_unsafe_fn)]
wit_bindgen::generate!({
    path: "./wit",
    world: "extension-world",
});

use exports::emporium::extensions::extension::{Guest, Instance, Metadata};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;

// Main component
struct Component;

// Messages that can be sent to the KV store
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase")]
enum Message {
    Get { key: String },
    Set { key: String, value: String },
    Delete { key: String },
}

// Simple Redis-like responses
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Response {
    Value(Option<String>), // GET returns the value or null
    Ok,                    // SET returns OK
    Deleted(u32),          // DELETE returns 1 or 0
}

#[derive(Default)]
struct KvStore(RefCell<Internal>);

#[derive(Default)]
struct Internal(HashMap<String, String>);

impl Internal {
    fn update(&mut self, msg: Message) -> Result<Response, String> {
        match msg {
            Message::Get { key } => Ok(Response::Value(self.0.get(&key).cloned())),
            Message::Set { key, value } => {
                self.0.insert(key, value);
                Ok(Response::Ok)
            }
            Message::Delete { key } => {
                let deleted = self.0.remove(&key).is_some();
                Ok(Response::Deleted(if deleted { 1 } else { 0 }))
            }
        }
    }
}

impl Guest for Component {
    type Instance = KvStore;

    fn get_metadata() -> Metadata {
        Metadata {
            id: "kv".to_string(),
            name: "Key-Value Extension".to_string(),
            version: "0.1.0".to_string(),
            description: "Stores values with SET and retrieves with GET operations".to_string(),
        }
    }
}

impl exports::emporium::extensions::extension::GuestInstance for KvStore {
    fn new(_config: String) -> Instance {
        log("info", "Creating new KV instance");
        Instance::new(KvStore::default())
    }

    fn update(&self, command: String) -> Result<String, String> {
        let msg: Message = serde_json::from_str(&command).map_err(|e| format!("Invalid message: {}", e))?;

        let response = self.0.borrow_mut().update(msg)?;
        serde_json::to_string(&response).map_err(|e| format!("Failed to serialize response: {}", e))
    }

    fn view(&self) -> String {
        let internal = self.0.borrow();
        json!({
            "type": "kv_store_info",
            "count": internal.0.len(),
            "keys": internal.0.keys().cloned().collect::<Vec<_>>()
        })
        .to_string()
    }
}

export!(Component);
