---
title: "Performance Tuning & Low-Level Optimization"
description: "Mechanical Sympathy principles: Arena, SIMD, cache optimization, and benchmark-driven workflow for P3 performance"
category: "Performance"
priority: "P3"
applies_to: ["standard", "strict"]
prerequisites: ["09-data-architecture.md", "11-concurrency.md"]
dependents: ["rust-systems-cloud-infra-guide/references/08-vectorized.md", "rust-systems-cloud-infra-guide/references/07-lock-free.md"]
---

# Performance Tuning & Low-Level Optimization: Mechanical Sympathy

> **Core Philosophy — Mechanical Sympathy (硬件同理心, Hardware-Aligned Optimization)**: Ultimate performance comes not from clever tricks, but from deep resonance between code logic and underlying physical hardware — CPU cache lines, pipelines, memory buses, and the kernel I/O stack.

> **📋 Document Profile**
> - **Domain**: P3 performance optimization (profiler-proven bottlenecks only)
> - **Priority**: P3 only (constrained by P0-P2)
> - **Modes**: `standard` (warning) → `strict` (mandatory benchmark proof)
> - **Prerequisites**: [`09-data-architecture.md`](09-data-architecture.md) (data layout), [`11-concurrency.md`](11-concurrency.md) (atomics)
> - **Deepened by**: [`08-vectorized.md`](../../rust-systems-cloud-infra-guide/references/08-vectorized.md) (SIMD intrinsics), [`07-lock-free.md`](../../rust-systems-cloud-infra-guide/references/07-lock-free.md) (lock-free structures)

---

## 0. Positioning & Priority Alignment

**This specification serves P3 (Extreme Performance) exclusively** — only activate when profiler has proven a code path is the system bottleneck.

### Execution Mode Integration

| Mode | Enforcement | Benchmark Requirement |
|------|-------------|----------------------|
| `rapid` | **Ignore all P3 rules** | None |
| `standard` | Warn on violations | Recommended |
| `strict` | **MUST attach benchmark report** | `criterion` comparison proving >5% gain |

**P3 Iron Law**: Any Arena introduction, SIMD manual implementation, lock-free transformation, or memory layout adjustment **MUST** include `criterion` benchmark comparison proving benefit > 5%.

### Scope Boundary

**This guide covers** (Universal Rust Projects):
- Diagnostic methodology and toolchain
- Benchmark-driven workflow
- Data layout principles (AoS → SoA)
- Basic atomic operations and memory ordering
- Auto-vectorization techniques
- When to consider advanced optimizations

**For cloud infrastructure scenarios** (database kernels, HFT, 10GbE+ gateways), refer to:
- [`rust-systems-cloud-infra-guide`](../rust-systems-cloud-infra-guide/README.md) for:
  - Hand-written SIMD and AVX-512 intrinsics
  - Lock-free data structures with Epoch reclamation
  - Arena allocators with Allocator API
  - NUMA-aware memory placement
  - Slab pre-allocation with mmap/mlock

---

## 1. Tuning Constitution: No Data, No Optimization

### 1.1 Release Mode Configuration

All benchmarks MUST run under `cargo build --release` with:

```toml
# Cargo.toml
[profile.release]
lto = true          # Link-Time Optimization: enables cross-crate inlining
codegen-units = 1   # Single codegen unit: maximizes inlining opportunities
```

**Why**: Default release settings use 16 codegen units for faster compilation, but this prevents cross-unit inlining and other optimizations.

### 1.2 Diagnostic Toolchain

| Tool | Purpose | Command Example |
|------|---------|----------------|
| **perf stat** | Hardware performance counters | `perf stat -e cache-misses,instructions ./target/release/app` |
| **flamegraph** | CPU hotspot visualization | `cargo flamegraph --bin app` |
| **heaptrack / dhat** | Heap allocation pattern analysis | `heaptrack ./target/release/app` |
| **cargo asm** | Inspect generated assembly | `cargo asm --lib my_crate::hot_function` |
| **criterion** | Micro-benchmark framework | `cargo bench --bench my_benchmark` |

### 1.3 Benchmark-Driven Workflow

```bash
# 1. Baseline measurement
cargo bench --bench my_benchmark -- --save-baseline main

# 2. Apply optimization

# 3. Compare against baseline
cargo bench --bench my_benchmark -- --baseline main

# Output:
# my_function             time:   [120.5 ns 121.2 ns 122.0 ns]
#                         change: [-5.2% -4.8% -4.3%] (p = 0.00 < 0.05)
#                         Performance has improved.
```

**Rule**: If improvement < 5%, revert optimization (not worth maintainability cost).

---

## 2. Memory Allocation Strategy

### 2.1 When to Consider Arena Allocation

**Activation Signal**: `perf` or `heaptrack` shows `malloc`/`free` occupying > 5% in hot path, AND allocated objects share unified lifecycle (e.g., AST nodes within single request, temporary graph structures).

**Diagnosis**: `flamegraph` shows `__libc_malloc` at top of hot path.

**Basic Pattern** (for universal projects):
```rust
use bumpalo::Bump;

fn handle_request() {
    let arena = Bump::new();
    
    // Allocation is just pointer movement - lock-free, no syscall
    let root = arena.alloc(parse_json(input));
    process(root);
    
    // When arena drops, all allocations reclaimed at once
}
```

**Benefit**: Reduces thousands of `malloc`/`free` calls to single pointer offset — O(1) allocation efficiency.

**For advanced scenarios** (per-request AST, NUMA-aware allocation, custom Allocator API):
→ See [`rust-systems-cloud-infra-guide/references/11-memory-advanced.md`](../rust-systems-cloud-infra-guide/references/11-memory-advanced.md)

### 2.2 Pre-allocation Discipline

**Rule**: Any construction involving `Vec`, `HashMap`, `String` must declare `with_capacity()` at initialization.

```rust
// ❌ Implicit reallocations
let mut vec = Vec::new();

// ✅ Pre-allocate with known capacity
let mut vec = Vec::with_capacity(1000);
```

### 2.3 Custom Allocator Evaluation

**For high-performance servers**, consider replacing global allocator:

```rust
use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
```

**Expected benefit**: Reduced fragmentation, improved concurrent throughput.

---

## 3. Data Layout: Aligning with Cache Physics

### 3.1 AoS to SoA Transformation

**Rule**: When iterating entire collections, refactor `Vec<Struct { a, b }>` to `Struct { a: Vec<..>, b: Vec<..> }`.

**Why**: CPU loads cache lines (64 bytes) contiguously. SoA ensures each cache line contains only needed data.

```rust
// ❌ AoS (Array of Structures) - cache pollution
struct Particle { x: f32, y: f32, z: f32, mass: f32 }
let particles: Vec<Particle>;

// When iterating only x coordinates:
for p in &particles {
    sum += p.x;  // Loads y, z, mass into cache unnecessarily
}

// ✅ SoA (Structure of Arrays) - cache resonance
struct Particles {
    x: Vec<f32>,
    y: Vec<f32>,
    z: Vec<f32>,
    mass: Vec<f32>,
}

// When iterating only x:
for &x in &particles.x {
    sum += x;  // Cache contains only x values
}
```

**Benefit**: 4x cache utilization improvement for single-field iteration.

### 3.2 Eliminate Pointer Chasing

**Prohibited**: `Vec<Box<T>>` or `Vec<Arc<T>>` on hot paths. Pointer chasing invalidates CPU prefetcher.

**MUST**: Use flat `Vec<T>` for contiguous memory layout.

```rust
// ❌ Pointer chasing - prefetcher fails
struct Engine {
    entities: Vec<Box<Entity>>  // Each dereference jumps to random heap location
}

// ✅ Contiguous memory - prefetcher works perfectly
struct Engine {
    entities: Vec<Entity>  // Data laid out sequentially in memory
}
```

**Diagnosis**: `perf stat -e cache-misses` shows high miss rate during iteration.

### 3.3 False Sharing Awareness

**Principle**: When two threads modify independent variables spaced < 64 bytes apart, CPU cache coherency protocol forces entire cache line to bounce between cores.

**Basic Defense**:
```rust
#[repr(align(64))]
struct AlignedCounter {
    counter: AtomicU64,
}
```

**For production systems** (using `CachePadded`, RCU patterns):
→ See [`rust-systems-cloud-infra-guide/references/07-lock-free.md`](../rust-systems-cloud-infra-guide/references/07-lock-free.md)

---

## 4. SIMD Vectorization

### 4.1 Trust Auto-Vectorization

**SHOULD**: First write branch-free, boundary-determined iterator chains to trigger LLVM auto-vectorization.

**Key Techniques**:
1. Use `assert!(a.len() == b.len())` to eliminate bounds checks
2. Use `step_by(4)` or similar to hint vectorization width
3. Avoid branches inside loops

```rust
fn add_vectors(a: &[f32], b: &[f32], out: &mut [f32]) {
    assert!(a.len() == b.len() && b.len() == out.len());
    
    // LLVM often auto-vectorizes this simple loop
    for i in 0..a.len() {
        out[i] = a[i] + b[i];
    }
}
```

**Verification**: `cargo asm --lib my_crate::add_vectors` to check for SIMD instructions (`vaddps`, `paddd`, etc.).

### 4.2 When to Consider Manual SIMD

**MAY**: When auto-vectorization fails AND benchmark proves necessity.

**For portable SIMD** (stable API):
```rust
use std::simd::{f32x8, SimdFloat};

fn simd_add(a: &[f32], b: &[f32], out: &mut [f32]) {
    let chunks = a.len() / 8;
    
    for i in 0..chunks {
        let va = f32x8::from_slice(&a[i * 8..]);
        let vb = f32x8::from_slice(&b[i * 8..]);
        let vc = va + vb;
        vc.copy_to_slice(&mut out[i * 8..]);
    }
    
    // Handle remainder
    for i in (chunks * 8)..a.len() {
        out[i] = a[i] + b[i];
    }
}
```

**P0 Red Line**: **MUST** use runtime feature detection and provide scalar fallback for architecture-specific intrinsics.

**For advanced SIMD** (AVX-512 intrinsics, bitmask branch elimination):
→ See [`rust-systems-cloud-infra-guide/references/08-vectorized.md`](../rust-systems-cloud-infra-guide/references/08-vectorized.md)

---

## 5. Atomic Operations & Memory Ordering

### 5.1 Basic Memory Ordering

**Default**: `Ordering::SeqCst` (safest, full memory barrier)

**Common Patterns**:
```rust
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// Simple counter (doesn't affect control flow)
static REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);

fn handle_request() {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
}

// Flag synchronization (producer-consumer)
static READY: AtomicBool = AtomicBool::new(false);

fn producer() {
    READY.store(true, Ordering::Release);
}

fn consumer() {
    while !READY.load(Ordering::Acquire) {}
}
```

### 5.2 When to Consider Lock-Free Structures

**MUST** use battle-tested crates for lock-free operations:
- `crossbeam-channel` / `flume` for lock-free queues
- `dashmap` for concurrent hash maps
- `crossbeam::epoch` for safe memory reclamation

**Why**: ABA problem, memory reclamation timing, and platform differences are extremely difficult to handle correctly.

**For advanced lock-free patterns** (RCU with arc-swap, Epoch-based reclamation, custom lock-free SkipList):
→ See [`rust-systems-cloud-infra-guide/references/07-lock-free.md`](../rust-systems-cloud-infra-guide/references/07-lock-free.md)

---

## 6. Advanced Optimizations (Reference Only)

### 6.1 Bounds Check Elimination

**Rule**: In profiler-verified extremely hot loops, if index safety is proven externally:

```rust
// SAFETY: i guaranteed in bounds by loop condition
for i in 0..vec.len() {
    let val = unsafe { *vec.get_unchecked(i) };
    process(val);
}
```

**Verification**: `cargo asm` shows removal of `cmp` + `jb` instructions.

### 6.2 Profile-Guided Optimization (PGO)

**For latency-critical services**:

```bash
# 1. Build with instrumentation
cargo rustc --release -- -C profile-generate=/tmp/pgo

# 2. Run representative workload
./target/release/app --workload representative

# 3. Build with profile data
cargo rustc --release -- -C profile-use=/tmp/pgo
```

**Expected gain**: 5-15% for latency-critical services.

### 6.3 BOLT: Post-Link Optimization

**For production binaries where LTO + PGO is already applied**:

BOLT (Binary Optimization and Layout Tool) reorders binary layout based on actual execution profiles, yielding 5-15% additional improvement beyond PGO.

```bash
# 1. Build with linker flags preserving relocations
RUSTFLAGS="-C link-arg=-Wl,--emit-relocs" cargo build --release

# 2. Profile with perf
perf record -e cycles:u -o /tmp/perf.data ./target/release/app --benchmark

# 3. Convert perf data to BOLT format
perf2bolt -p /tmp/perf.data ./target/release/app -o /tmp/perf.fdata

# 4. Optimize binary
llvm-bolt ./target/release/app -o ./target/release/app.bolt \
    --data=/tmp/perf.fdata \
    --reorder-blocks=ext-tsp \
    --reorder-functions=hfsort+ \
    --split-functions \
    --split-all-cold \
    --dyno-stats \
    --hot-func-cutoff-max=32768
```

**When to apply BOLT**:
- Binary size > 100MB where I-cache misses dominate
- Long-running server applications (not CLI tools)
- After PGO + LTO shows diminishing returns

**Red Line**: BOLT is a **strict mode only** optimization. Requires LLVM 16+ and Linux perf. Not applicable to `wasm32` targets.

### 6.4 PGO + BOLT Combined Pipeline

```yaml
# CI pipeline order: PGO → BOLT → final binary
optimizations:
  - LTO (codegen-units=1, lto=fat)
  - PGO (profile-generate → collect → profile-use)
  - BOLT (perf record → perf2bolt → llvm-bolt)
  - Verify: benchmark comparison vs baseline
```

### 6.3 Advanced Topics

For the following advanced scenarios, refer to cloud infrastructure guide:

| Topic | Universal Guide | Cloud Infrastructure Guide |
|-------|----------------|---------------------------|
| **Arena Allocation** | Basic `bumpalo` usage | Allocator API, NUMA-aware, Slab pre-allocation |
| **SIMD** | Auto-vectorization, portable SIMD | AVX-512 intrinsics, bitmask parsing |
| **Lock-Free** | Basic atomics, crossbeam crates | RCU, Epoch reclamation, custom structures |
| **Memory Ordering** | SeqCst, Relaxed, Acquire/Release | Fine-grained ordering proofs with loom |
| **Cache Alignment** | `#[repr(align(64))]` | `CachePadded`, false sharing elimination |

→ See [`rust-systems-cloud-infra-guide/README.md`](../rust-systems-cloud-infra-guide/README.md)

---

## 7. Integration with Overall Architecture

### 7.1 Design Philosophy Alignment

| Principle | Performance Application |
|-----------|------------------------|
| **Mechanical Sympathy** | Data layout aligned with cache lines |
| **Economy of Motion** | Pre-allocation, arena for unified lifecycle |
| **Flow Like Water** | SoA layout adapts to CPU prefetcher |

### 7.2 Priority Constraints

**P3 is constrained by**:
- **P0 (Correctness)**: Never introduce UB for performance
- **P1 (Maintainability)**: If optimization < 5%, prefer clarity

### 7.3 Terminology Links

All specialized terms defined in [`glossary.md`](05-glossary.md):
- Cache Line
- False Sharing
- Bump Allocation
- SIMD
- Memory Ordering
- Pointer Chasing
- SoA vs AoS

### 7.4 Testing Integration

From [`advanced-testing.md`](26-advanced-testing.md):
- **Loom**: Systematically explore thread interleavings for lock-free code
- **Miri**: Detect UB in unsafe blocks
- **cargo-fuzz**: Fuzz test parsers before optimization

---

## 8. Agent Performance Checklist

Before applying any P3 optimization:

### Prerequisites
- [ ] Profiler data confirms this is bottleneck (flamegraph, perf)
- [ ] Baseline benchmark established (`criterion`)
- [ ] Optimization target quantified (>5% improvement)

### Memory & Allocation
- [ ] Hot path `format!` or `.to_string()` eliminated?
- [ ] `Vec`/`HashMap` pre-allocated with capacity?
- [ ] Arena considered for unified-lifecycle objects?
- [ ] Custom allocator evaluated (jemallocator/mimalloc)?

### Data Layout
- [ ] SoA conversion for bulk iteration?
- [ ] Pointer chasing (`Vec<Box<T>>`) eliminated?
- [ ] False sharing prevented (align 64)?

### SIMD & Vectorization
- [ ] Auto-vectorization verified (`cargo asm`)?
- [ ] Manual SIMD has runtime feature detection?
- [ ] Scalar fallback provided?

### Concurrency
- [ ] Lock-free structures use battle-tested crates?
- [ ] Memory ordering justified (not blind SeqCst)?
- [ ] Loom verification for lock-free code?

### Safety
- [ ] All `unsafe` blocks have `SAFETY` comments?
- [ ] Miri verification for unsafe code?
- [ ] P0 correctness preserved?

---

## 9. Trade-offs & Decision Framework

### When NOT to Optimize

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| Cold path (config, init) | **Skip** | P1 > P3: clarity wins |
| Improvement < 5% | **Revert** | Not worth maintainability cost |
| Adds significant complexity | **Reject** | P1 > P3 unless bottleneck critical |
| No profiler data | **Defer** | No data, no optimization |

### Progressive Optimization Path

```
rapid (MVP)
├─ Use String, Vec, standard allocators
└─ No P3 optimizations

↓ (Profiler identifies bottleneck)

standard (Production)
├─ Apply targeted optimizations
├─ Document with // P3: reason comments
└─ Benchmark reports attached

↓ (Extreme performance required)
    AND cloud infrastructure scenario

strict (Ultra-low latency)
├─ Arena allocation with Allocator API
├─ SIMD manual vectorization (AVX-512)
├─ Lock-free data structures with Epoch
├─ PGO enabled
└─ Full benchmark suite + perf reports
    ↓
    Refer to rust-systems-cloud-infra-guide
```

---

## 10. Related

### Within rust-architecture-guide
- [`00-mode-guide.md`](00-mode-guide.md) — Execution modes governing P3 enforcement
- [`01-priority-pyramid.md`](01-priority-pyramid.md) — Priority framework (P0-P3)
- [`05-glossary.md`](05-glossary.md) — Terminology: cache line, false sharing, SIMD, etc.
- [`11-concurrency.md`](11-concurrency.md) — Lock strategies, atomic operations
- [`26-advanced-testing.md`](26-advanced-testing.md) — Loom, Miri, fuzz testing

### rust-systems-cloud-infra-guide (Advanced Scenarios)
- [`07-lock-free.md`](../rust-systems-cloud-infra-guide/references/07-lock-free.md) — RCU, Epoch reclamation, memory ordering proofs
- [`08-vectorized.md`](../rust-systems-cloud-infra-guide/references/08-vectorized.md) — SIMD intrinsics, bitmask parsing, SoA columnar
- [`11-memory-advanced.md`](../rust-systems-cloud-infra-guide/references/11-memory-advanced.md) — Arena allocators, Slab, NUMA, Allocator API

---

**Version History**: 
- v2.0 — Clarified scope boundary with cloud infrastructure guide, added diagnostic toolchain, mode layering, anti-pattern diagnosis
- v1.0 — Initial mechanical sympathy specification
