---
name: rust-wasm-frontend-infra-guide
description: >
  Hard constraints for Rust code targeting wasm32-unknown-unknown, covering compilation
  configuration, cross-language boundary, linear memory management, concurrency model,
  and general code adaptation. This Skill is the compilation & boundary layer foundation
  for all Rust+Wasm frontend architectures (including no-DOM rendering engines) and must
  be inherited and obeyed.
license: MIT
metadata:
  version: "4.1.0"
  philosophy: "Dialectical Materialism & Jeet Kune Do — Wasm Vertical Base"
  domain: "wasm32-unknown-unknown compilation & boundary layer"
  relationship: "vertical-deepening-of:rust-architecture-guide"
  rust_edition: "2024"
  aligned_with: ["Leptos Binary Size Guide", "twiggy diagnostics", "wasm-opt -Oz", "talc allocator", "Wasm Component Model proposals"]
---

# Rust to Wasm Vertical Compilation & Boundary Specification V4.1.0

This specification inherits the core philosophy from the Rust Architecture Guide and the Rust Project Lifecycle Guide, deepening vertically for the uniqueness of the `wasm32-unknown-unknown` target (linear memory, single-threaded event loop, cross-language boundary).

V4.1.0 — **Binary Size Economics & Ecosystem Convergence**:
- Aligns with Leptos official binary size optimization guide: streaming WASM compilation rationale, `twiggy` top functions analysis
- Recommends `talc` allocator (replacing deprecated `wee_alloc`) with quantitative baseline (~10KB → ~1KB)
- References Wasm Component Model proposals for future canonical ABI alignment
- Adds `wasm-tools` stripping and `wasm-objdump` size auditing to the toolchain

## Architectural Philosophy: Dialectical Materialism and Jeet Kune Do with Unity of False and Real

This specification is the engineering implementation of the "Dialectical Materialism and Jeet Kune Do" architectural philosophy at the compilation and boundary layer. Legal weight: This specification is the final criterion and thinking framework for all technical decisions, code reviews, and architectural evolution. Any concrete Skill or implementation specification that conflicts with this creed must itself be revised.

### 0.1 Materialist Foundation: Reverence for Physical Boundaries

- **Linear memory only grows**, must be reclaimed internally.
- **FFI boundary is expensive RPC**, any implicit conversion is a performance liability.
- **Browser is a sandboxed OS**, event loop, multi-threading limits, and security policies are physical laws that must be complied with.

### 0.2 Dialectical Core: Embrace and Transform Contradictions

Core contradiction: JS's **dynamism (flexible, GC, untyped)** vs Rust's **determinism (static, no GC, strongly typed)**.

Architectural solution: Through **unidirectional control (Rust is the brain, JS is the dumb brush)**, isolate the two sides. The Rust side pursues extreme determinism, the JS side pursues extreme pure execution. Neither pollutes the other.

### 0.3 Jeet Kune Do Essence: Economy, Directness, and Unity of False and Real

- **Economy**: Retain only necessary bytes, instructions, and abstraction layers. `opt-level = "z"` is the embodiment of this spirit at the compilation level.
- **Directness**: Pursue the shortest physical execution path. Zero-copy views are the exemplar of "real", directly hitting the performance bottleneck of data transfer.
- **Unity of False and Real**: The "false" move is not redundant; it is the setup, probing, and rhythm control for the final "real" move. Strong typing constraints, crash-preventing error handling, and phased code organization are the "false" — they consume a small number of bytes and engineer's mental effort, but exchange them for system **determinism, robustness, and maintainability**, creating the stage for the "real" to perform. Without the false, the real has no foundation.

→ [references/08-philosophy-v2.md](references/08-philosophy-v2.md)

## Iron Rules Derived from Materialist Foundation

| ID | Iron Rule | Meaning |
|----|-----------|---------|
| **IRON-01** | Binary Size Is Paramount | Compilation artifacts must be aggressively compressed, all debug info, unwind tables, and redundant sections stripped |
| **IRON-02** | Zero-Copy at Boundary | On high-frequency data paths, always pass pointer+length, never serialize; zero-copy view lifetimes must be documented |
| **IRON-03** | Memory Partitioning | Global state static residency, per-frame objects ephemeral, prevent dynamic allocation fragmentation |
| **IRON-04** | Cross-Origin Isolation Documented | Any use of `SharedArrayBuffer` must document COOP/COEP header configuration requirements |

→ [references/01-iron-rules.md](references/01-iron-rules.md)

---

## Tiered Defense: Memory Management Philosophy

| Tier | Applicable | Weapon |
|------|------------|--------|
| Tier 1: Per-Frame Ephemeral | Not exceeding a single `rAF` callback | Bump Allocator (`bumpalo`) |
| Tier 2: Scene-Level Residency | Interaction scene lifecycle (drag, modal) | Nestable or independent Arena pools |
| Tier 3: Global Orthogonal Storage | Cross entire application lifecycle | `SlotMap`, ECS-style component storage |

→ [references/08-philosophy-v2.md](references/08-philosophy-v2.md) §1

## Architecture Decision Tree: When Faced with Dilemmas

1. **Closer to physical reality?** — Prefer more direct, predictable control over bottom-level memory and instructions, no implicit "magic"
2. **Does the "false" serve a more fundamental "real"?** — Prefer overhead that clearly exchanges for core value
3. **Compliant with browser physical laws?** — Prefer not fighting the host environment, leverage its characteristics

→ [references/08-philosophy-v2.md](references/08-philosophy-v2.md) §2

## Code Review "Smells"

| Smell | Philosophical Deviation | Correction |
|-------|------------------------|------------|
| Unjustified `.unwrap()` | Lazy to lay the "false" error handling net | `Result<T, JsValue>` or `expect` with clear message |
| High-frequency `serde_json` | "False" move in wrong place | `WasmSlice` zero-copy |
| Exported functions return `String`/`Vec` | Ignorance of FFI boundary cost | Return `WasmSlice` (pointer+length) |
| Business logic `if/else` on JS side | Brain leaking to brush | Logic centralized in Rust, JS pure execution only |
| Abandoning all debugging for zero overhead | Rigid "realism", ignoring "false" value | Retain `tracing_wasm` + `console_error_panic_hook` |

→ [references/08-philosophy-v2.md](references/08-philosophy-v2.md) §4

---

## Stage 1: Compilation & Artifact Control

- **1.1 Release Profile (MUST)**: `opt-level="z"`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true`
- **1.2 Post-Processing Optimization (MUST)**: `wasm-opt -Oz` (Binaryen v110+), typically reduces an additional 15-20%
- **1.3 Allocator Replacement (SHOULD)**: Deprecate `wee_alloc`, recommend `talc` or `MiniAlloc`, baseline from ~10KB to ~1KB

→ [references/02-build-control.md](references/02-build-control.md)

---

## Stage 2: FFI & Cross-Language Boundary

- **2.1 Zero-Copy Memory Views (MUST)**: `WasmSlice` safe encapsulation template, JS-side mandatory consumption within tick
- **2.2 Boundary Security Risks**: Dangling pointer risk, lifetimes must be documented in `# Safety` comments
- **2.3 Explicit Boundary Type Contracts (MUST)**: Parameters must be scalars or `WasmSlice`, prohibit `JsValue` pass-through

→ [references/03-ffi-boundary.md](references/03-ffi-boundary.md)

---

## Stage 3: Memory & Lifecycle

- **3.1 Lifecycle Separation (MUST)**: Global residency (pre-allocated `Vec`/`ArrayVec`/`SlotMap`) vs per-frame ephemeral (`bumpalo` Arena + per-frame `reset()`)
- **3.2 Physical Defense Against Memory Leaks (MUST)**: Explicit `.free()` on JS component destruction, `WeakRef` auxiliary, `beforeunload` global cleanup

→ [references/04-memory-lifecycle.md](references/04-memory-lifecycle.md)

---

## Stage 4: Concurrency & Event-Driven

- **4.1 Seamless Async Mapping (MUST)**: `wasm-bindgen-futures` (`JsFuture` + `spawn_local`)
- **4.2 Prohibit Blocking APIs (MUST NOT)**: No `std::thread::sleep`, blocking `Mutex::lock()`, busy-waiting
- **4.3 Heavy Computation Worker Isolation (SHOULD)**: COOP/COEP prerequisites, `#[repr(C)]` shared memory, `AtomicU32`/`AtomicU64` synchronization

→ [references/05-concurrency-events.md](references/05-concurrency-events.md)

---

## Stage 5: General Code Wasm Adaptation

- **5.1 Error Handling (MUST)**: `Result<T, JsValue>` + `thiserror`, under `panic="abort"` must use `Result` not `panic!`
- **5.2 Logging & Observability (MUST)**: `console_error_panic_hook::set_once()` + `tracing_wasm::set_as_global_default()`

→ [references/06-wasm-adaptation.md](references/06-wasm-adaptation.md)

---

## Prohibitions & Compliance Self-Check

7 hard prohibitions + 10-item compliance self-check list.

→ [references/07-prohibitions-checklist.md](references/07-prohibitions-checklist.md)

---

## Reference Files

| File | Topic | Key Directive |
|------|-------|---------------|
| [01-iron-rules.md](references/01-iron-rules.md) | Iron Rules | IRON-01~04 iron rules and their physical basis |
| [02-build-control.md](references/02-build-control.md) | Compilation & Artifact Control | `Cargo.toml` release profile + `wasm-opt` + allocator replacement |
| [03-ffi-boundary.md](references/03-ffi-boundary.md) | FFI & Cross-Language Boundary | `WasmSlice` zero-copy encapsulation + explicit type contracts |
| [04-memory-lifecycle.md](references/04-memory-lifecycle.md) | Memory & Lifecycle | Global residency vs per-frame ephemeral + Arena `reset()` + leak defense |
| [05-concurrency-events.md](references/05-concurrency-events.md) | Concurrency & Event-Driven | `wasm-bindgen-futures` + blocking prohibition + Worker isolation |
| [06-wasm-adaptation.md](references/06-wasm-adaptation.md) | General Code Wasm Adaptation | `Result<T, JsValue>` + `console_error_panic_hook` + `tracing_wasm` |
| [07-prohibitions-checklist.md](references/07-prohibitions-checklist.md) | Prohibitions & Compliance | 7 hard prohibitions + 10-item self-check list |
| [08-philosophy-v2.md](references/08-philosophy-v2.md) | Architectural Philosophy & Decision Meta-Spec | Dialectical Materialism + Jeet Kune Do + tiered defense + decision tree + review smells |
| [09-zero-copy-pool.md](references/09-zero-copy-pool.md) | Zero-Copy Resource Pool | Resource pool topology, JS injection via `encodeInto`, Wasm resolution with boundary interception, frame lifecycle sync |
| [10-command-bus-v3.md](references/10-command-bus-v3.md) | Zero-Copy Command Bus V3.1 | Double-buffer topology, atomic synchronization (`AcqRel`), Facade write/consume cycle, lifecycle safety contract, hard constraints [F-08]~[F-11] |
| [11-toolchain-v3.md](references/11-toolchain-v3.md) | Toolchain & Lifecycle Automation V3.2 | Layout assertions, binary size budget, twiggy diagnostics, performance telemetry, lifecycle checkpoints, hard constraints [F-12]~[F-15] |
| [12-domain-engines-v4.md](references/12-domain-engines-v4.md) | Domain Engines V4.1 | Pixel rendering (wgpu/vello/tiny-skia), vector & search (SIMD128), high-density state (CRDT/yrs), time slicing, environment purity, hard constraints [F-16]~[F-20] |
| [13-cross-platform-isomorphism-v5.md](references/13-cross-platform-isomorphism-v5.md) | Cross-Platform Isomorphism V5.1 | Component model vs native dual-track, capability Trait abstraction, SIMD dispatch (`edgevec`/`archmage`), atomic semantic alignment, hard constraints [F-21]~[F-24] |

---

## Changelog

### V4.1.0
- Version bumped to align with universal constitution V9.1.0
- Added binary size diagnostics: twiggy top/dominators/paths workflow, binary size budget tiers, wasm-tools strip, wasm-objdump audit
- Recommends `talc` allocator (replacing deprecated `wee_alloc`) with quantitative baseline (~10KB → ~1KB)
- Aligns with Leptos official binary size optimization guide including streaming WASM compilation rationale
- References Wasm Component Model proposals for future canonical ABI alignment

### V4.0.0
- Version bumped to align with universal constitution V9.0.0 restructuring
- Philosophy updated to "Dialectical Materialism & Jeet Kune Do — Unity of False and Real"
- Maintains alignment with memory layout transparency, breakwater pattern, and physical feasibility audit paradigms (no new documents required — V3.0.0 already covers the three dimensions through IRON rules, FFI boundary specs, and memory lifecycle)
