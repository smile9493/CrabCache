---
title: "Concurrency & Event-Driven"
description: "Async model mapping, blocking API prohibition, Worker isolation and SharedArrayBuffer prerequisites"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md"]
dependents: []
---

# Stage 4: Concurrency & Event-Driven (Concurrency & Event Loop)

> **Iron Rule Basis**: [IRON-04] Cross-Origin Isolation Documented

The browser main thread must not be blocked; the event loop must be complied with.

---

## 4.1 Seamless Async Mapping (MUST)

All I/O operations must use `wasm-bindgen-futures`, which provides `JsFuture` (Promise to Future) and `future_to_promise` (Future to Promise) bidirectional conversion.

```rust
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
pub fn start_network_task() {
    spawn_local(async {
        let result = fetch_data().await;
        update_state(result);
    });
}
```

---

## 4.2 Prohibit Blocking APIs (MUST NOT)

- Forbidden: `std::thread::sleep`, `std::sync::Mutex::lock()` (freezes tab on contention)
- Forbidden: Any form of busy-waiting loop
- Single Wasm function calls exceeding ~5ms should consider chunking or offloading to Worker

---

## 4.3 Heavy Computation Worker Isolation (SHOULD)

Before using Worker + `SharedArrayBuffer`, the following prerequisites must be evaluated:

1. **COOP/COEP Server Header Configuration**: `SharedArrayBuffer` requires the page to be in a "cross-origin isolated" state. Servers must set:
   - `Cross-Origin-Opener-Policy: same-origin`
   - `Cross-Origin-Embedder-Policy: require-corp`
   - **If deploying on platforms that don't support custom HTTP headers (e.g., GitHub Pages), a Service Worker injection workaround is needed**, but this has limited compatibility and must be noted in the architecture decision record.
2. Shared memory layout must be defined using `#[repr(C)]` structs with explicitly calculable field offsets.
3. Multi-Worker synchronization must use `std::sync::atomic` (`AtomicU32`, `AtomicU64`), combined with `Atomics.wait` / `Atomics.notify`.
4. **Fallback**: If COOP/COEP is not feasible, using `postMessage` with ownership transfer (Transferables) is a degraded alternative, not zero-copy.
