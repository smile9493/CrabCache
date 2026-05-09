---
title: "Architectural Philosophy & Decision Meta-Spec V2.0"
description: "Dialectical Materialism and Jeet Kune Do core creed, tiered defense, architecture decision tree, code review smells"
category: "Philosophical"
priority: "P0"
applies_to: ["rapid", "standard", "strict"]
prerequisites: ["01-iron-rules.md"]
dependents: []
---

# Architectural Philosophy & Decision Meta-Spec V2.0

> **Legal Weight**: This specification is the final criterion and thinking framework for all technical decisions, code reviews, and architectural evolution. Any concrete Skill or implementation specification that conflicts with this creed must itself be revised.

---

## 0. Core Creed: Dialectical Materialism and Jeet Kune Do with Unity of False and Real

### 0.1 Materialist Foundation: Reverence for Physical Boundaries

We revere physical reality and abhor magic and implicit overhead. Any architectural decision must be based on the following unshakeable physical facts:

- **Linear memory only grows**: Wasm module heap space is a finite physical resource.
- **FFI boundary is expensive RPC**: Every call and parameter passing between JS and Wasm has quantifiable cost.
- **Browser is a sandboxed OS**: Event loop, multi-threading limits, and security policies (COOP/COEP) are physical laws that must be complied with.

**Engineering Elaboration**: See [01-iron-rules.md](01-iron-rules.md) (physical basis of the four iron rules)

---

### 0.2 Dialectical Core: Embrace and Transform Contradictions

We do not attempt to eliminate contradictions in the system; instead, through architectural design, we let the unity of opposites drive the system to a higher equilibrium.

- **Core Contradiction**: JS's **dynamism (flexible, GC, untyped)** vs Rust's **determinism (static, no GC, strongly typed)**.
- **Architectural Solution**: Through **unidirectional control (Rust is the brain, JS is the dumb brush)**, isolate the two sides. The Rust side pursues extreme determinism, the JS side pursues extreme pure execution. Neither pollutes the other.

**Engineering Elaboration**: See [03-ffi-boundary.md](03-ffi-boundary.md) §2.3 (explicit boundary type contracts), [05-concurrency-events.md](05-concurrency-events.md) §4.3 (Worker isolation)

---

### 0.3 Jeet Kune Do Essence: Economy, Directness, and Unity of False and Real

- **Economy (Economy of Motion)**: Retain only necessary bytes, instructions, and abstraction layers. `opt-level = "z"` is the embodiment of this spirit at the compilation level.
- **Directness**: Pursue the shortest physical execution path. Zero-copy views are the exemplar of "real", directly hitting the performance bottleneck of data transfer.
- **Unity of False and Real**: The "false" move is not redundant; it is the setup, probing, and rhythm control for the final "real" move. Strong typing constraints, crash-preventing error handling, and phased code organization are the "false" — they consume a small number of bytes and engineer's mental effort, but exchange them for system **determinism, robustness, and maintainability**, creating the stage for the "real" to perform. Without the false, the real has no foundation.

**Engineering Elaboration**: See [02-build-control.md](02-build-control.md) §1.1 (`opt-level="z"`), [03-ffi-boundary.md](03-ffi-boundary.md) §2.1 (`WasmSlice` zero-copy)

---

## 1. Tiered Defense: Application of Memory Management Philosophy

For large-scale applications, we introduce a **tiered defense** strategy, allocating resources with the thinking of unity of false and real:

### 1.1 Tier 1: Per-Frame Ephemeral

- **Applicable**: Transient data whose lifetime does not exceed a single `rAF` callback.
- **Weapon**: Bump Allocator (`bumpalo`).
- **Unity of False and Real**: `bumpalo`'s instantaneous allocation and bulk reset is a "real" performance optimization that eliminates GC jitter. But `bumpalo` itself is a "false" container — it sacrifices some memory flexibility in exchange for absolute determinism within the frame loop.

### 1.2 Tier 2: Scene-Level Residency

- **Applicable**: Data valid within a single interaction scene (e.g., a drag operation, a modal dialog lifecycle).
- **Weapon**: Nestable or independent Arena pools.
- **Unity of False and Real**: This is a "false" move to prevent the "real" per-frame move from causing memory buildup due to long-lifetime object escape. It introduces an intermediate layer to manage complexity, ensuring the purity of the first tier defense.

### 1.3 Tier 3: Global Orthogonal Storage

- **Applicable**: Core state spanning the entire application lifecycle.
- **Weapon**: `SlotMap`, ECS-style component storage.
- **Unity of False and Real**: Entity IDs are "false" indices — they are not data themselves. This indirection (false) achieves complete decoupling of data and logic (real), fundamentally eliminating Rust's thorny reference lifetime problems.

**Engineering Elaboration**: See [04-memory-lifecycle.md](04-memory-lifecycle.md) §3.1 (lifecycle separation)

---

## 2. Architecture Decision Tree: When Faced with Dilemmas

When faced with two seemingly correct technical options, judge in this order:

**First Question: Is it closer to physical reality?**

- Which option provides more direct, predictable control over bottom-level memory and instructions?
- *Prefer the option that is more transparent to the hardware/runtime environment, without implicit "magic".*

**Second Question: Does the "false" serve a more fundamental "real"?**

- Which option's extra overhead (type gymnastics, intermediate layers, abstractions) ensures system determinism, robustness, or maintainability, rather than purposeless over-engineering?
- *Prefer the option whose "false" stands on solid ground — its overhead can clearly explain what core value it exchanges for.*

**Third Question: Is it compliant with the browser's physical laws?**

- Which option better conforms to the event loop, security policies, etc.?
- *Prefer the option that does not fight the host environment but leverages its characteristics.*

---

## 4. Code Review "Smells"

The following smells are signals of philosophical deviation and must be caught during review:

| Smell | Philosophical Deviation | Correction |
|-------|------------------------|------------|
| **Unjustified `.unwrap()`** | Lazy to lay the "false" error handling net, especially evil at boundaries — can crash the entire instance | Use `Result<T, JsValue>` or `expect` with clear message |
| **Serialization overhead on hot paths (`serde_json`)** | "False" move in the wrong place, paying huge parsing cost for readability, violates performance "real" requirement | Use `WasmSlice` zero-copy direct transfer |
| **Exported functions returning `String` or `Vec`** | Ignorance of the FFI boundary, treating expensive implicit conversions as nothing | Return `WasmSlice` (pointer+length) |
| **Business logic `if/else` on JS side** | Brain function leaking to the brush, destroying the fundamental "real" of unidirectional control | Centralize logic in Rust, JS only does pure execution |
| **Abandoning all debugging capability for zero overhead** | Rigid "realism", not understanding the value of "false", will eventually pay a huge price in production debugging | Retain `tracing_wasm` and `console_error_panic_hook` |

**Engineering Elaboration**: See [06-wasm-adaptation.md](06-wasm-adaptation.md) §5.1 (error handling), [03-ffi-boundary.md](03-ffi-boundary.md) §2.1 (`WasmSlice`)

---

## 5. Attack Directive Confirmation

Based on this V2.0 meta-specification, the next attack direction: **Binary Encoding Specification for Cross-Language Command Bus**.

This will be a classic unity of false and real engineering:

- **Real**: Pursue the most compact binary layout, so the JS brush can read instructions without any loss.
- **False**: Design a lean but extensible instruction header, handling version and length "meta-information", leaving room for the long-term evolution of the command bus.
