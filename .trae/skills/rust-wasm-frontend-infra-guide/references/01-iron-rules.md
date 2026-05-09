---
title: "Iron Rules"
description: "Constitutional foundation of the Rust-Wasm frontend infrastructure vertical deepening, four iron rules and their physical basis"
category: "Constitutional"
priority: "P0"
applies_to: ["rapid", "standard", "strict"]
prerequisites: []
dependents: ["02-build-control.md", "03-ffi-boundary.md", "04-memory-lifecycle.md", "05-concurrency-events.md", "06-wasm-adaptation.md", "07-prohibitions-checklist.md"]
---

# Core Philosophy and the Unshakeable Iron Rules

This specification is the engineering implementation of the "Dialectical Materialism and Jeet Kune Do" architectural philosophy at the compilation and boundary layer. It is based on the following physical facts:

- **Wasm linear memory only grows**, must be reclaimed internally.
- **JS to Wasm boundary is an expensive RPC**, any implicit conversion is a performance liability.
- **Browser main thread must not be blocked**, must comply with the event loop.
- **Zero-copy views are unsafe operations**, lifetimes must be constrained by specification.

From these, four iron rules are derived:

---

## [IRON-01] Binary Size Is Paramount

Compilation artifacts must be aggressively compressed, all debug info, unwind tables, and redundant sections stripped.

**Physical Basis**: WASM binaries must be downloaded, compiled, and instantiated on every page load. Every byte directly impacts first load time. Production configurations of mainstream projects like Leptos, NEAR, and SWC all use this as baseline.

**Engineering Elaboration**: See [02-build-control.md](02-build-control.md)

---

## [IRON-02] Zero-Copy at Boundary

On high-frequency data paths, always pass pointer+length, never serialize; zero-copy view lifetimes must be documented.

**Physical Basis**: Each JS to Wasm boundary crossing costs ~100ns. In a render loop processing 10,000 elements, per-attribute FFI calls (10 attributes x 10,000 elements) consume 10ms — an entire frame budget wasted on bridge overhead.

**Engineering Elaboration**: See [03-ffi-boundary.md](03-ffi-boundary.md)

---

## [IRON-03] Memory Partitioning

Global state static residency, per-frame objects ephemeral, prevent dynamic allocation fragmentation.

**Physical Basis**: Wasm linear memory only grows and cannot return memory to the OS. Fragmentation from dynamic allocation accumulates over time, eventually causing memory pressure. Arena allocators achieve O(1) lifecycle management through per-frame batch reclamation.

**Engineering Elaboration**: See [04-memory-lifecycle.md](04-memory-lifecycle.md)

---

## [IRON-04] Cross-Origin Isolation Documented

Any use of `SharedArrayBuffer` must document COOP/COEP header configuration requirements.

**Physical Basis**: `SharedArrayBuffer` requires the page to be in a "cross-origin isolated" state. Servers must set `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. If deploying on platforms that don't support custom HTTP headers (e.g., GitHub Pages), a Service Worker injection workaround is needed, but compatibility is limited.

**Engineering Elaboration**: See [05-concurrency-events.md](05-concurrency-events.md)

---

## Relationships Between Iron Rules

```
IRON-01 Binary Size Is Paramount
    │
    ├── IRON-02 Zero-Copy at Boundary (less serialization = less size + less overhead)
    │       │
    │       └── IRON-03 Memory Partitioning (zero-copy requires stable memory lifetimes)
    │               │
    │               └── IRON-04 Cross-Origin Isolation (multi-threaded shared memory requires documented safety boundary)
    │
    └── All subsequent rules are elaborations of these four iron rules
```

All subsequent rules are elaborations of these four iron rules.
