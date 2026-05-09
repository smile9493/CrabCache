---
title: "Compilation & Artifact Control"
description: "Cargo.toml release profile, wasm-opt post-processing optimization, allocator replacement selection, twiggy diagnostics, binary size budgeting"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md"]
dependents: ["11-toolchain-v3.md"]
aligned_with: ["Leptos Binary Size Guide", "twiggy diagnostics", "wasm-opt -Oz"]
---

# Stage 1: Compilation & Artifact Control

> **Iron Rule Basis**: [IRON-01] Binary Size Is Paramount

---

## 1.1 Release Profile (MUST)

In `Cargo.toml`, the following release configuration must be used for Wasm builds and must not be casually overridden:

```toml
[profile.release]
opt-level = "z"       # Size priority (or "s" if performance benchmarks show significant difference)
lto = true            # Force fat LTO, maximize dead code elimination
codegen-units = 1     # Single compilation unit, give optimizer global view
panic = "abort"       # Disable stack unwinding, reduce size and avoid UB in browser unwinding
strip = true          # Strip symbols, further compress
```

**Basis**: Leptos, NEAR, SWC, and other mainstream projects all use this as baseline configuration, validated to effectively reduce binary size.

---

## 1.2 Post-Processing Optimization (MUST)

The final step of the build pipeline must use `wasm-opt` (Binaryen toolkit v110+) for binary-level optimization:

```bash
wasm-opt -Oz --enable-simd -o output.wasm input.wasm
```

- `-Oz` for ultimate size optimization, typically reduces an additional 15-20%.
- `--enable-simd` optional, decide based on target browser compatibility.
- Must verify in CI that file size regression after `wasm-opt` does not exceed budget.

---

## 1.3 Allocator Replacement (SHOULD)

The choice of global allocator directly affects the .wasm baseline size:

- **Deprecation Warning**: `wee_alloc` has been unmaintained for a long time, **new projects must not use it**.
- **Recommended**: `talc` (modern, actively maintained) or `MiniAlloc` (Wasm-optimized, uses zero-filled pages to reduce memory footprint).
- **Benefit**: Default allocator is ~10KB, replacement can reduce to ~1KB, the most direct way to shrink baseline size.

Decision records must include volume benchmark results for the chosen allocator.

```toml
[dependencies]
talc = "4"

[features]
default = ["talc_allocator"]
talc_allocator = []
```

```rust
#[cfg(feature = "talc_allocator")]
#[global_allocator]
static ALLOCATOR: talc::TalckWasm = unsafe { talc::TalckWasm::new_global() };
```
