# Rust to Wasm Vertical Compilation & Boundary Specification

[![Version](https://img.shields.io/badge/Version-v4.0.0-purple.svg)]()
[![Reference Docs](https://img.shields.io/badge/Reference-13%20Docs-blue.svg)]()
[![Domain](https://img.shields.io/badge/Domain-Wasm%20Vertical%20Base-9b59b6.svg)]()

**Rust-Wasm Frontend Infrastructure Vertical Deepening Architectural Specification** — Hard constraints for `wasm32-unknown-unknown` target compilation configuration, cross-language boundary, linear memory management, concurrency model, and general code adaptation.

---

## Overview

This specification inherits the core philosophy from [`rust-architecture-guide`](../rust-architecture-guide/), deepening vertically for the uniqueness of the `wasm32-unknown-unknown` target (linear memory, single-threaded event loop, cross-language boundary).

This Skill is the compilation & boundary layer foundation for all Rust+Wasm frontend architectures (including no-DOM rendering engines) and must be inherited and obeyed.

**Environment Assumptions**: Wasm linear memory only grows, JS to Wasm boundary is expensive RPC, browser main thread must not be blocked, zero-copy views are unsafe operations.

## Core Philosophy

This specification is the engineering implementation of the "Dialectical Materialism and Jeet Kune Do" architectural philosophy at the compilation and boundary layer.

- **Wasm linear memory only grows**, must be reclaimed internally.
- **JS to Wasm boundary is an expensive RPC**, any implicit conversion is a performance liability.
- **Browser is a sandboxed OS**, event loop, multi-threading limits, and security policies are physical laws that must be complied with.

From these, four iron rules are derived:

| ID | Iron Rule | Meaning |
|----|-----------|---------|
| **IRON-01** | Binary Size Is Paramount | Compilation artifacts must be aggressively compressed, all debug info, unwind tables, and redundant sections stripped |
| **IRON-02** | Zero-Copy at Boundary | On high-frequency data paths, always pass pointer+length, never serialize; zero-copy view lifetimes must be documented |
| **IRON-03** | Memory Partitioning | Global state static residency, per-frame objects ephemeral, prevent dynamic allocation fragmentation |
| **IRON-04** | Cross-Origin Isolation Documented | Any use of `SharedArrayBuffer` must document COOP/COEP header configuration requirements |

---

## Document Index

`references/` directory contains **12 reference documents**, strictly corresponding to the specification's stages:

### I. Iron Rules

| # | Document | Coverage |
|---|----------|----------|
| **01** | [iron-rules.md](references/01-iron-rules.md) | Four iron rules IRON-01~04 and their physical basis, relationships between iron rules |

### II. Compilation & Artifact Control

| # | Document | Coverage |
|---|----------|----------|
| **02** | [build-control.md](references/02-build-control.md) | `Cargo.toml` release profile (MUST) + `wasm-opt` post-processing (MUST) + allocator replacement (SHOULD) |

### III. FFI & Cross-Language Boundary

| # | Document | Coverage |
|---|----------|----------|
| **03** | [ffi-boundary.md](references/03-ffi-boundary.md) | `WasmSlice` zero-copy safe encapsulation (MUST) + boundary security risks + explicit type contracts (MUST) |

### IV. Memory & Lifecycle

| # | Document | Coverage |
|---|----------|----------|
| **04** | [memory-lifecycle.md](references/04-memory-lifecycle.md) | Lifecycle separation: global residency vs per-frame ephemeral (MUST) + physical defense against memory leaks (MUST) |

### V. Concurrency & Event-Driven

| # | Document | Coverage |
|---|----------|----------|
| **05** | [concurrency-events.md](references/05-concurrency-events.md) | Async model mapping (MUST) + blocking API prohibition (MUST NOT) + Worker isolation & SharedArrayBuffer (SHOULD) |

### VI. General Code Wasm Adaptation

| # | Document | Coverage |
|---|----------|----------|
| **06** | [wasm-adaptation.md](references/06-wasm-adaptation.md) | Error handling `Result<T, JsValue>` + `thiserror` (MUST) + logging `console_error_panic_hook` + `tracing_wasm` (MUST) |

### VII. Prohibitions & Compliance Self-Check

| # | Document | Coverage |
|---|----------|----------|
| **07** | [prohibitions-checklist.md](references/07-prohibitions-checklist.md) | 7 hard prohibitions [F-01]~[F-07] + 10-item compliance self-check list |

### VIII. Architectural Philosophy & Decision Meta-Spec V2.0

| # | Document | Coverage |
|---|----------|----------|
| **08** | [philosophy-v2.md](references/08-philosophy-v2.md) | Dialectical Materialism core creed + Jeet Kune Do unity of false and real + tiered defense + architecture decision tree + code review smells |

### IX. Zero-Copy Resource Pool

| # | Document | Coverage |
|---|----------|----------|
| **09** | [zero-copy-pool.md](references/09-zero-copy-pool.md) | Resource pool topology (static + transient segments) + JS injection via `TextEncoder.encodeInto` + Wasm resolution with boundary interception + frame lifecycle synchronization + hard constraints [F-05]~[F-07] |

### X. Zero-Copy Command Bus V3.1

| # | Document | Coverage |
|---|----------|----------|
| **10** | [command-bus-v3.md](references/10-command-bus-v3.md) | Double-buffer topology (`DoubleBuffer` header, 16 bytes) + atomic synchronization (`AcqRel` ordering) + JS Facade write cycle (`DataView` scalar injection) + Wasm safe consume loop (single slice dispatch) + lifecycle safety contract + hard constraints [F-08]~[F-11] |

### XI. Toolchain & Lifecycle Automation V3.2

| # | Document | Coverage |
|---|----------|----------|
| **11** | [toolchain-v3.md](references/11-toolchain-v3.md) | Compile-time layout assertions (`size_of` + `offset_of`) + binary size budget (`.wasm-size-budget.json`) + `twiggy` diagnostics + `performance.now()` telemetry + lifecycle checkpoints + hard constraints [F-12]~[F-15] |

### XII. Domain Engines V4.1

| # | Document | Coverage |
|---|----------|----------|
| **12** | [domain-engines-v4.md](references/12-domain-engines-v4.md) | Three domain engines: pixel rendering (wgpu/vello/tiny-skia tier selection), vector & search (SIMD128 + macerator), high-density state (CRDT/yrs mandatory + dependency injection) + cooperative time slicing (8ms threshold) + environment purity principle + hard constraints [F-16]~[F-20] |

---

## Relationship

```
rust-architecture-guide (Universal Constitution)
          │
          └──► rust-wasm-frontend-infra-guide (Vertical Deepening)
                      │
                      ├── IRON-01 Binary Size → Compilation & Artifact Control
                      ├── IRON-02 Zero-Copy  → FFI & Cross-Language Boundary
                      ├── IRON-03 Partitioning → Memory & Lifecycle
                      ├── IRON-04 Isolation  → Concurrency & Event-Driven
                      │
                      ├── General Code Wasm Adaptation
                      └── Prohibitions + Compliance Self-Check
```

- This guide depends on `rust-architecture-guide`'s P0-P3 priority framework and execution modes
- This guide adds compilation & boundary layer red lines for `wasm32-unknown-unknown` scenarios on top of P0 safety
- **Complementary use**: The universal constitution provides the priority framework; this guide is the Wasm vertical deepening amendment

---

## File Structure

```
rust-wasm-frontend-infra-guide/
├── SKILL.md                          # Skill entry (Agent instructions)
├── README.md                         # Document index
└── references/                       # 12 reference documents
    ├── 01-iron-rules.md              # Iron Rules
    ├── 02-build-control.md           # Compilation & Artifact Control
    ├── 03-ffi-boundary.md            # FFI & Cross-Language Boundary
    ├── 04-memory-lifecycle.md        # Memory & Lifecycle
    ├── 05-concurrency-events.md      # Concurrency & Event-Driven
    ├── 06-wasm-adaptation.md         # General Code Wasm Adaptation
    ├── 07-prohibitions-checklist.md  # Prohibitions & Compliance Self-Check
    ├── 08-philosophy-v2.md           # Architectural Philosophy & Decision Meta-Spec
    ├── 09-zero-copy-pool.md          # Zero-Copy Resource Pool
    ├── 10-command-bus-v3.md          # Zero-Copy Command Bus V3.1
    ├── 11-toolchain-v3.md            # Toolchain & Lifecycle Automation V3.2
    └── 12-domain-engines-v4.md       # Domain Engines V4.1
```

---

## License

[MIT](../LICENSE)
