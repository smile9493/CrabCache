---
title: "Domain Engines V4.1"
description: "Vertical deepening specification for domain-specific engines: pixel-perfect rendering, vector & search, high-density state computation, with SIMD/time-slicing/dependency injection compliance"
category: "Architecture"
priority: "P0"
version: "4.1.0"
related:
  - "01-iron-rules.md (IRON-02: Zero-Copy at Boundary)"
  - "03-ffi-boundary.md (WasmSlice)"
  - "04-memory-lifecycle.md (Arena Lifecycle)"
  - "10-command-bus-v3.md (Command Bus)"
  - "11-toolchain-v3.md (Toolchain & Lifecycle)"
  - "../rust-architecture-guide/references/30-memory-layout.md (Memory Layout Transparency)"
---

# Domain Engines Vertical Deepening Specification V4.1

## 0. Philosophical Foundation & Trigger Criteria

Domain engines are the ultimate "real strike" of infrastructure deepening. They are only permitted when pure JS optimization cannot break through the following physical bottlenecks:

- **GC Escape**: Managing millions of long-lifecycle objects where JS frequent GC pauses cause frame-rate collapse.
- **Hardware Utilization**: Must directly leverage WASM SIMD instruction sets or WebGPU compute shaders.
- **Binary-Native**: The data stream itself is a compact binary protocol (PDF, custom compression formats, etc.), requiring in-place parsing and computation — any serialization is an unacceptable loss.

**Philosophical Mapping**: When conditions are met, we must strike like Jeet Kune Do's "direct attack" — using the lowest-level hardware instructions to complete computation on the shortest path, without any detour.

## 1. Three Domain Engine Implementation Protocols

### 1.1 Pixel-Perfect Rendering Engine

**Objective**: Completely bypass the browser DOM/CSS, driving `<canvas>` or `OffscreenCanvas` directly from linear memory.

**Tier Selection**:

| Tier | Crate | Status | Use Case |
|------|-------|--------|----------|
| **Production-Ready** | `wgpu` | Stable | 3D scenes, large-scale data visualization, high-performance games — near-native GPU control in browser |
| **Evaluating (Alpha)** | `vello` | Alpha (v0.7.0 as of 2026.01) | Figma-level infinite canvas — acceptable if team can tolerate evolution risk |
| **Lightweight Conservative** | `tiny-skia` | Stable | Pure CPU rasterization — no SIMD dependency, simple and reliable, suitable for low-complexity 2D |

**Memory Protocol**:
- Vertex buffers and index buffers must reside persistently on the Wasm side and be reused — absolutely prohibited from creating per-frame.
- Pass buffer base addresses to WebGL / WebGPU via `WasmSlice`.

**Unity of False and Real**:
- JS only sends commands like `DrawPrimitive` through the command bus.
- Pipeline state and shader binding are all completed within the Wasm kernel.

### 1.2 Vector & Search Engine

**Objective**: Sub-millisecond high-dimensional vector similarity retrieval or full-text search within the browser sandbox.

**SIMD Execution Path**:
- Functions must be annotated with `#[target_feature(enable = "simd128")]` — this is a compile-time commitment. Wasm32 does not allow runtime SIMD availability detection.
- Recommended: Use `macerator` as a cross-platform SIMD abstraction layer (covering x86_64, aarch64, wasm32), or directly call `core::arch::wasm32` built-in functions.

**Data Alignment**:
- Wasm's `v128_load` supports unaligned loads at the instruction level, so **16-byte alignment is not forced**.
- However, it is **strongly recommended (SHOULD)** to align vector data to 16 bytes for optimal performance on most hardware backends and to guarantee safety when simultaneously compiling to x86 or other targets.

**Index Layout**:
- All index structures must be marked `#[repr(C)]`.
- Map the entire inverted or vector index as a read-only view to JS via `WasmSlice` for debugging or auxiliary operations.
- The computation core must never involve JS.

### 1.3 High-Density State Engine

**Objective**: Handling undo/redo with tens of thousands of nodes, real-time multi-end collaboration conflict resolution (CRDT), complex state machines.

**CRDT Reuse Priority**: **Mandatory**. Must reuse the community-proven `yrs` (Yjs Rust port) rather than building CRDT logic from scratch. It provides complete solutions including Text, Array, Map, sub-documents, and Undo manager — battle-tested.

**Environment Dependency Injection**: Non-deterministic values such as timestamps must absolutely never call `SystemTime::now()` directly in core logic (which panics in Wasm). Must be injected from the JS side via FFI, e.g., `js_sys::Date::now()`. This ensures the computation kernel remains a pure, reproducible function.

**Flattened Storage**: Use `SlotMap` or pre-allocated `Vec` instead of interconnected reference pointers, reducing snapshot and rollback overhead to O(1) segment operations. JS side serves only as a "view projection" — any state mutation must occur within Wasm.

## 2. Core Layer "Environment Purity" Principle (SHOULD)

| Principle | Description |
|-----------|-------------|
| **Pure Computation Isolation** | Engine state machines, SIMD kernels, encoders/decoders must never directly depend on `web-sys` or any browser-specific Web API |
| **No Mandatory `no_std`** | Under `wasm32-unknown-unknown`, the standard library's `Vec`, `String`, `Box`, etc., are fully available and well-optimized. Only consider removing `std` when pursuing extreme size reduction (e.g., < 10KB) and when a global allocator can be independently handled |
| **Environment Interaction Through Bus** | All environment interactions (network, time, clipboard) must pass through the bus or dependency injection interfaces, preserving the core's testability and cross-platform capability |

## 3. Cooperative Time Slicing (MUST)

If a single computation task is estimated to exceed **8ms** (more than half a frame budget at 60fps), cooperative suspension must be implemented.

**Implementation**: Reuse `n0_future`'s `yield_now()` or a custom future based on `wasm-bindgen-futures`. Within the computation loop, call `.await` after processing every N elements, actively returning control to the browser event loop.

**Strictly Prohibited**: Busy-waiting or any form of blocking operation on the main thread.

```rust
// Example: Cooperative time slicing in a heavy computation loop
use n0_future::task::yield_now;

pub async fn heavy_computation(data: &[u8]) -> Result<Vec<u32>, EngineError> {
    let mut result = Vec::with_capacity(data.len() / 4);
    let mut processed = 0;

    for chunk in data.chunks(1024) {
        // Process chunk...
        processed += chunk.len();

        // Yield every N chunks to avoid blocking the main thread
        if processed % 4096 == 0 {
            yield_now().await;
        }
    }

    Ok(result)
}
```

## 4. Hard Constraints

| ID | Constraint | Rationale |
|----|-----------|-----------|
| **[F-16] No Dynamic Branching in SIMD** | SIMD computation cores must not use dynamic branches — separate SIMD-able paths by batch as much as possible | Dynamic branches destroy SIMD parallelism and cause pipeline stalls |
| **[F-17] #[repr(C)] for Shared/SIMD Structs** | All structs shared with JS or used in SIMD must be `#[repr(C)]` and validated with compile-time assertions for size and offset | Layout drift causes silent data corruption across language boundaries |
| **[F-18] No Long-Lived JsValue References** | The core must never hold long-lived `JsValue` references — external resources must be converted to numeric handles | `JsValue` retention prevents GC and creates cross-heap coupling |
| **[F-19] No Reinvention of Mature Crates** | Must not reimplement functionality already provided by mature crates like `yrs` within domain engines, especially CRDT | Reinventing CRDT is a multi-year effort — reuse is the only rational choice |
| **[F-20] No SystemTime in Wasm** | Must not call `SystemTime::now()` directly within Wasm to obtain timestamps — must use dependency injection | `SystemTime::now()` panics on `wasm32-unknown-unknown` without proper feature flags |

## 5. Compliance Self-Check List

- [ ] **Trigger Rationality**: Has a pure JS approach been fully evaluated, confirming physical limits before launching a domain engine?
- [ ] **SIMD Lockdown**: Are all SIMD acceleration paths correctly annotated with `simd128` and gated behind compile-time feature flags?
- [ ] **Rendering Selection**: Has a clear choice been made between `wgpu` (production) / `vello` (experimental) / `tiny-skia` (conservative) based on project risk tolerance?
- [ ] **Library Reuse Rate**: Has CRDT integrated `yrs`, and has vector retrieval evaluated `macerator`?
- [ ] **Environment Isolation**: Are non-deterministic inputs (timestamps, random numbers) injected only through FFI?
- [ ] **Task Slicing**: Do all heavy computation tasks that may exceed 8ms include `yield_now()` logic?
- [ ] **Layout Interception**: Do domain structures have compile-time `assert_layout` or test-validated sizes and offsets?
- [ ] **Memory Audit**: Are rendering buffers reused, and are index/vector data avoiding unnecessary reallocations?

---

This specification marks the complete transformation of frontend infrastructure "from false to real": JS degrades into a pure UI rendering shell and input event collector, while the Rust domain engine becomes the true physical brain. At this point, browser applications first achieve the physical feasibility to support heavy industrial-grade software (CAD, video editing, large-scale simulation).
