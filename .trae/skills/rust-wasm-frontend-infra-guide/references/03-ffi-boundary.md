---
title: "FFI & Cross-Language Boundary"
description: "Zero-copy memory view safe encapsulation, boundary security risks, explicit contracts for boundary types"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md"]
dependents: ["06-wasm-adaptation.md"]
---

# Stage 2: FFI & Cross-Language Boundary (JS-Wasm Boundary)

> **Iron Rule Basis**: [IRON-02] Zero-Copy at Boundary

---

## 2.1 Zero-Copy Memory Views (MUST)

When handling audio, video, image pixels, or large matrices, zero-copy memory views must be used instead of any serialization scheme.

**Safe Encapsulation Template** (must be followed):

```rust
use wasm_bindgen::prelude::*;

/// Safe encapsulation for zero-copy views.
/// # Safety
/// - Caller must ensure `ptr` and `len` originate from valid Wasm linear memory.
/// - JS-side TypedArray view must be consumed within this tick, must not be saved across frames.
#[wasm_bindgen]
pub struct WasmSlice {
    ptr: *const u8,
    len: usize,
}

#[wasm_bindgen]
impl WasmSlice {
    pub fn ptr(&self) -> *const u8 { self.ptr }
    pub fn len(&self) -> usize { self.len }

    /// Create a zero-copy view from a Rust slice.
    /// Caller is responsible for ensuring the slice lives long enough in the original memory.
    pub fn from_slice(data: &[u8]) -> WasmSlice {
        WasmSlice {
            ptr: data.as_ptr(),
            len: data.len(),
        }
    }
}
```

**JS-Side Mandatory Consumption Protocol**:

```javascript
const view = wasm.get_frame_data();
const buffer = new Float64Array(wasm.memory.buffer, view.ptr(), view.len() / 8);
// Consume immediately within this tick, do not store to closures, async callbacks, or global variables
gl.bufferData(gl.ARRAY_BUFFER, buffer, gl.DYNAMIC_DRAW);
```

---

## 2.2 Boundary Security Risks

- **Dangling Pointer Risk**: If Rust frees heap data while JS still holds a TypedArray view, it will read invalid memory or crash. Rust's ownership model cannot automatically protect across FFI boundaries — lifetimes must be enforced through specification constraints.
- **Lifetime Convention**: All zero-copy view documentation comments must include a `# Safety` section declaring the view's validity period (typically "valid within this frame" or "valid until next reset").

---

## 2.3 Explicit Boundary Type Contracts (MUST)

All structs exposed to JS must use the `#[wasm_bindgen]` macro, and parameters must be explicit scalars or `WasmSlice`.

```rust
// FORBIDDEN: relying on implicit serialization across boundary
#[wasm_bindgen]
pub fn process_data(data: &JsValue) { ... }

// CORRECT: explicit scalar passing
#[wasm_bindgen]
pub fn process_config(flag: u32, threshold: f64) { ... }

// CORRECT: zero-copy for large data paths
#[wasm_bindgen]
pub fn get_large_buffer() -> WasmSlice { ... }
```
