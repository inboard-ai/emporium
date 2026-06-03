// A key-value store Emporium extension written in JavaScript.
//
// The point of this crate is to prove the host↔extension boundary is the WIT/ABI
// contract, not Rust: this component is built with `jco` from plain JS, exports
// the same `emporium:extensions@0.3.0` `tool-extension` world the Rust extensions
// do, and the host loads and drives it identically.
//
// `list_keys` returns a data frame whose Arrow IPC buffer is produced by the
// pure-JavaScript `apache-arrow` library — so even the columnar data wire is
// written by a non-Rust stack and read by the host's polars decoder.

import { Table, Utf8, vectorFromArray, tableToIPC } from "apache-arrow";

// Components are single-instance and single-threaded — a module-level Map is the
// whole store.
const STORE = new Map();

export const extension = {
  getMetadata() {
    return {
      id: "kv-js",
      name: "Key-Value Store (JavaScript)",
      version: "0.1.0",
      description: "A kv store extension written in JavaScript — proof the WIT is the contract, not the language.",
    };
  },
  init(_config) {
    // No configuration; accept anything. (void return = ok)
  },
  view() {
    return `kv-js: ${STORE.size} key(s)`;
  },
};

function text(content) {
  return { tag: "text-output", val: { content } };
}

export const toolProvider = {
  listTools() {
    const info = (id, name, description, schema) => ({
      id,
      name,
      description,
      schema,
      cacheable: false,
      activity: undefined,
      examples: [],
    });
    return [
      info(
        "set",
        "Set",
        "Store a value by key",
        '{"type":"object","properties":{"key":{"type":"string"},"value":{"type":"string"}},"required":["key","value"]}',
      ),
      info(
        "get",
        "Get",
        "Retrieve a value by key",
        '{"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}',
      ),
      info("list_keys", "List Keys", "List all keys as a data frame", '{"type":"object","properties":{}}'),
    ];
  },

  executeTool(name, params) {
    let parsed;
    try {
      parsed = params ? JSON.parse(params) : {};
    } catch (e) {
      throw `invalid params JSON: ${e}`;
    }

    if (name === "set") {
      const { key, value } = parsed;
      if (typeof key !== "string" || typeof value !== "string") {
        throw "set requires string 'key' and 'value'";
      }
      STORE.set(key, value);
      return text("OK");
    }

    if (name === "get") {
      const { key } = parsed;
      if (typeof key !== "string") throw "get requires string 'key'";
      return text(STORE.has(key) ? STORE.get(key) : "(not found)");
    }

    if (name === "list_keys") {
      const keys = [...STORE.keys()].sort();
      // Pure-JS Arrow: force a Utf8 column (so an empty store still types as
      // string) and serialize to the Arrow IPC *file* format the host reads.
      const column = vectorFromArray(keys, new Utf8());
      const table = new Table({ Key: column });
      const arrowIpc = tableToIPC(table, "file"); // Uint8Array

      return {
        tag: "data-frame-output",
        val: {
          schema: [{ name: "key", alias: "Key", dtype: "string" }],
          arrowIpc,
        },
      };
    }

    throw `unknown tool: ${name}`;
  },
};
