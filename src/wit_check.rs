// Phase 1 smoke check: verifies wasmtime can parse the new WIT.
// This file is deleted in Phase 2 when real bindgen usage replaces it.
#[allow(dead_code)]
mod check {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "tool-extension",
        imports: { default: async },
        exports: { default: async },
    });
}
