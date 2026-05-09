---
title: "General Code Wasm Adaptation"
description: "Error handling (Result<T, JsValue>), logging and observability (console_error_panic_hook + tracing_wasm)"
category: "Code Style"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md", "03-ffi-boundary.md"]
dependents: []
---

# Stage 5: General Code Wasm Adaptation

---

## 5.1 Error Handling (MUST)

`Result<T, E>` across the FFI boundary must be converted to `Result<T, JsValue>`. It is recommended to derive the `Error` trait using `thiserror`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("invalid command: {0}")]
    InvalidCommand(u32),
    #[error("state conflict")]
    StateConflict,
}

#[wasm_bindgen]
pub fn execute_command(cmd: u32) -> Result<(), JsValue> {
    core_logic(cmd).map_err(|e| JsValue::from_str(&e.to_string()))
}
```

**Regarding Recovery Under `panic = "abort"`**: Experience from Cloudflare Workers shows that `wasm-bindgen` treats panic as unrecoverable — after a panic, the entire Wasm instance is considered in an invalid state. Therefore, business logic must use `Result` rather than `panic!` to handle foreseeable failure paths.

---

## 5.2 Logging & Observability (MUST)

```rust
// During Wasm module initialization, the following dual configuration must be called:
use console_error_panic_hook;
use tracing_wasm;

#[wasm_bindgen(start)]
pub fn init() {
    // Step 1: Convert Rust panic to formatted console messages, avoiding unreadable
    // "RuntimeError: unreachable executed" errors
    console_error_panic_hook::set_once();

    // Step 2: Configure tracing to output structured logs to the browser console
    // Note: verify compatibility of the tracing-wasm version with the tracing ecosystem
    tracing_wasm::set_as_global_default();
}
```

**Basis**: `console_error_panic_hook` forwards panic messages to `console.error`; `tracing-wasm` previously bound to older `tracing` API versions, so version compatibility must be confirmed when selecting.
