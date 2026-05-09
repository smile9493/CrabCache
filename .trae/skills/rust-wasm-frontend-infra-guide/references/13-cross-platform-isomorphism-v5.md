---
title: "Cross-Platform Physical Isomorphism V5.1"
description: "Cross-platform isomorphism specification: component model vs native path dual-track strategy, SIMD dispatch, atomic semantic alignment, and platform capability trait abstraction"
category: "Architecture"
priority: "P0"
version: "5.1.0"
related:
  - "01-iron-rules.md (IRON-02: Zero-Copy at Boundary)"
  - "03-ffi-boundary.md (WasmSlice)"
  - "10-command-bus-v3.md (Command Bus)"
  - "12-domain-engines-v4.md (Domain Engines)"
  - "../rust-architecture-guide/references/30-memory-layout.md (Memory Layout Transparency)"
  - "../rust-architecture-guide/references/31-breakwater-pattern.md (Breakwater Architecture)"
---

# Cross-Platform Physical Isomorphism Specification V5.1

## 0. Philosophical Foundation

- **Dialectical Materialism**: Acknowledge that physical differences across platforms (browser sandbox, mobile permission models, desktop system calls) cannot be eliminated. Rather than forcibly flattening these differences, architectural design isolates them in extremely thin adaptation layers that do not pollute each other.
- **Unity of False and Real**:
  - **Real**: Core domain engines (Layer 4) remain pure computation, zero platform dependencies, compiled to deterministic Wasm or native binaries.
  - **False**: Platform adaptation layers (Glue Code) use Trait abstractions and conditional compilation as "false moves," providing unified internal interfaces for different hosts.
- **Kernel Monism**: Not code sharing (a false abstraction), but physical semantic consistency (a real strike) — the same core logic produces identical computational results on both Web and Native ends.

## 1. Dual-Track Isomorphism Strategy

### 1.1 Component Model Track (Forward-Looking, Evaluating)

Wasm Component Model + WIT is the long-term standard for cross-language type-safe communication. Evaluate introduction in the following scenarios:

- Project is primarily server-side/edge computing, with browser as secondary.
- Multi-language composition needed (Rust + Python + JS) with stable inter-component interfaces.
- Team can accept the evolution risk of the `cargo-component` toolchain.

**Current Limitations**: Browsers do not natively support the component model. Components must be transpiled to ES modules via `jco`, and its WASI browser support is labeled "experimental." The WASI roadmap does not target the browser — native browser support is expected to take considerable time.

### 1.2 Native Path (Production-Ready)

Current production recommendation:

| Platform | Strategy |
|----------|----------|
| **Web** | `wasm32-unknown-unknown` + `#[wasm_bindgen]` + shared linear memory (zero-copy command bus V3.1) |
| **Desktop** | Native Rust binary, driving window and GPU directly via `winit`/`wgpu` |
| **Mobile** | Compile Rust core as Wasm module, embed in lightweight runtimes like Wasmtime/Wasm3, bridge platform APIs via FFI |

Both tracks share identical core engine code — the only difference lies in the platform adaptation layer implementations.

## 2. Platform Adaptation Layer Design

### 2.1 Capability Abstraction (Trait Injection)

The core layer declares required external capabilities via Traits, with each platform providing concrete implementations:

```rust
// core/src/traits.rs — Core layer defines, zero platform dependencies
pub trait HostIO: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
    fn now_millis(&self) -> u64;
    fn log(&self, level: LogLevel, msg: &str);
}
```

**Key Correction**: What is prohibited here is "direct invocation of platform-specific APIs" (e.g., `web_sys::window()` or `CreateFileW` on Windows), not the mere referencing of crates like `winapi`. The `winapi` crate compiles to no-op on non-Windows platforms, and with `#[cfg(target_os = "windows")]` isolation, the core layer can safely depend on it. The true red line is **leaking platform-specific capability calls**, not crate dependency declarations.

### 2.2 Memory Layout Consistency

All cross-platform shared structures must be marked `#[repr(C)]` and validated through compile-time assertions in CI for consistent size and offset:

```rust
#[repr(C)]
pub struct CrossPlatformDTO {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub flags: u32,
}

#[test]
fn verify_dto_layout() {
    assert_eq!(std::mem::size_of::<CrossPlatformDTO>(), 24);
}
```

**Verification**: In CI, compile for both `wasm32-unknown-unknown` and `x86_64-unknown-linux-gnu` targets, running the same layout tests.

## 3. SIMD Graded Compatibility

Use `edgevec::simd_dispatch!` or `archmage::#[arcane]` macros for compile-time SIMD auto-dispatch:

```rust
use edgevec::simd_dispatch;

simd_dispatch! {
    #[inline]
    pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
        wasm_simd: wasm128::dot_product(a, b),
        avx2: unsafe { x86_avx2::dot_product(a, b) },
        neon: neon::dot_product(a, b),
        fallback: scalar::dot_product(a, b),
    }
}
```

**Principle**: All SIMD paths are compile-time determined with zero runtime branching. The same source file generates `simd128` instructions on Wasm, AVX2 on x86_64, and NEON on aarch64 — computational density is physically equivalent.

## 4. Atomic Semantic Alignment

### 4.1 Main Thread Blocking Trap

Calling `Atomics.wait` on the browser main thread panics directly. Concurrent primitive semantics are not interchangeable across platforms:

| Platform | Blocking Lock Behavior |
|----------|----------------------|
| Browser Main Thread | `Atomics.wait` → panic |
| Browser Worker | `Atomics.wait` → normal blocking |
| Native Desktop | `std::sync::Mutex::lock()` → system-level blocking |

### 4.2 Unified Adaptation Solution

Use `wasm_safe_mutex` or build a custom adaptation layer that automatically selects the correct lock strategy per platform at compile time:

| Platform | Lock Strategy |
|----------|--------------|
| **Wasm Main Thread** | Spin lock (`spin_loop`), never call `Atomics.wait` |
| **Wasm Worker Thread** | `Atomics.wait` + `Atomics.notify`, efficient blocking |
| **Native Desktop** | `std::sync::Mutex`, full system-level support |

Core layer concurrent code uses the same Trait interface for locks, with the adaptation layer injecting the corresponding implementation at compile time.

## 5. Hard Constraints

| ID | Constraint | Rationale |
|----|-----------|-----------|
| **[F-21] No Platform API in Core** | Must not directly call `std::fs`, `std::net`, `web_sys::window()`, or other platform-specific APIs in the core layer. Must inject through Trait abstractions. | Platform API calls create hard dependencies that break cross-platform compilation |
| **[F-22] No Host-Dependent Endianness** | Cross-end communication protocols must enforce little-endian byte order, consistent with the Wasm physical standard. | Endianness mismatch causes silent data corruption across platforms |
| **[F-23] No Fat Pointers at FFI Boundary** | Must not pass `&dyn Trait` or `Box<dyn Trait>` across FFI boundaries. Must convert to opaque handles (`u32`/`u64`). | Fat pointer layout is platform-specific and ABI-unstable |
| **[F-24] No Atomics.wait on Main Thread** | Must not call `Atomics.wait` or any synchronous primitive that depends on it on the browser main thread. Must use `wasm_safe_mutex` or equivalent adaptation. | `Atomics.wait` panics immediately on the browser main thread |

## 6. Compliance Self-Check List

- [ ] **Dual-Track Strategy**: Has the appropriate isomorphism track been chosen based on the project's primary battlefield (Web-first vs Server-first)?
- [ ] **Component Testing**: If the component model track is chosen, can the kernel run independently in a Wasmtime environment without a browser, via WASI interfaces?
- [ ] **Memory Alignment Consistency**: Are `size_of` and field offsets of core DTOs completely equal under both `x86_64` native and `wasm32` targets?
- [ ] **Adaptation Layer Thickness**: Is the platform adaptation layer code volume less than 5% of the core engine code volume?
- [ ] **Rendering Instruction Replay**: Can the same binary instruction stream render pixel-consistent results in both browser Canvas and native desktop windows?
- [ ] **SIMD Coverage**: Do core computation paths have corresponding SIMD implementations under all three targets: `wasm32`, `x86_64`, `aarch64`?
- [ ] **Atomic Safety**: Have all lock operations been confirmed to use spin locks rather than `Atomics.wait` on the browser main thread?
- [ ] **Dependency Audit**: Does the core layer `Cargo.toml` exclude `web-sys`? If `winapi` is depended upon, is it isolated via conditional compilation?

---

This specification marks the fifth layer of frontend infrastructure evolution: **Kernel Monism, Cross-Platform Physical Isomorphism** — fully aligned with the Rust community's "Write Once, Run Anywhere via Wasm" practice. The V5.1 specification:

1. Positions the component model as a "forward-looking track" rather than a "current mandate," avoiding over-investment in immature infrastructure for browser scenarios.
2. Provides concrete community tools (`edgevec::simd_dispatch!`, `archmage`) for SIMD grading, translating abstract principles into executable code patterns.
3. Reveals the extremely specific physical limitation of `Atomics.wait` panic on the browser main thread, and provides the `wasm_safe_mutex` solution.
4. Corrects the technical deviation in the `winapi` prohibition statement — the prohibition target is explicitly "direct invocation of platform-specific APIs," not "referencing specific crates."
