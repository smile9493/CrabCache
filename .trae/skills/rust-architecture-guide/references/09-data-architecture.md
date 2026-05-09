---
title: "Data Architecture & Memory Management"
description: "Zero-copy parsing, Arena allocation, stack-first principles, and capacity management for Rust data structures"
category: "Architecture"
priority: "P1-P3"
applies_to: ["rapid", "standard", "strict"]
prerequisites: []
dependents: ["rust-systems-cloud-infra-guide/references/11-memory-advanced.md"]
---

# Data Architecture & Memory Management: Zero-Overhead Principles

> **Core Philosophy — Intercepting Boilerplate (拦截样板, Zero-Copy Design)**: Don't move data, just point to where it is. Zero-copy and stack-first designs improve both performance and predictability.

> **📋 Document Profile**
> - **Domain**: All Rust data structure design
> - **Priority**: P1 (maintainability) → P3 (performance optimization)
> - **Modes**: `rapid` (allow clone) → `standard` (prefer zero-copy) → `strict` (mandatory Miri/dhat)
> - **Prerequisites**: None (foundational document)
> - **Deepened by**: [`11-memory-advanced.md`](../../rust-systems-cloud-infra-guide/references/11-memory-advanced.md) (NUMA, Allocator API)

---

## 0. Positioning & Priority Alignment

### Priority Framework

- **Primary: P1 (Maintainability)**: Zero-copy and stack-first improve code predictability and reduce cognitive load.
- **P3 Performance Acceleration**: Arena allocation, `Bytes`, and capacity pre-judgment are **enforced only when profiler proves allocation is the bottleneck**.

### Execution Mode Integration

| Mode | Enforcement | Requirements |
|------|-------------|--------------|
| `rapid` | **Relaxed** | Direct `.clone()` and `String` allowed; can ignore `Cow`, but encouraged to keep ownership clear |
| `standard` | **Full rules** | All rules active; exceptions must be commented with justification |
| `strict` | **Enhanced** | **Mandatory** zero-copy, Arena, and capacity checks; CI includes `dhat` heap analysis and `cargo miri` safety verification |

---

## 1. Zero-Copy Parsing: Building Instant Views

> **Philosophy — Zero-Copy (零拷贝, Reference-First Design)**: Don't move data, just point to where it is.

### 1.1 Lifetime Projection

**MUST**: When extracting fields from input buffers (e.g., DPF/JSON), default to returning `&str` or `&[u8]` (borrowed views), not owned data.

**Exception**: If fields are numerous and must outlive the buffer (e.g., cross-thread传递), allow `Arc<str>` or `Bytes`, with justification comment.

**Good Example**:
```rust
// ✅ Zero-copy view
struct Record<'a> {
    id: &'a str,
    payload: &'a [u8],
}

fn parse_record(data: &[u8]) -> Record {
    // Return borrowed references, no allocation
    Record {
        id: std::str::from_utf8(&data[0..8]).unwrap(),
        payload: &data[8..],
    }
}
```

**When to Own**:
```rust
// ✅ Allowed: Field must outlive buffer
use std::sync::Arc;

struct CachedRecord {
    id: Arc<str>,  // Comment: Shared across threads
    payload: bytes::Bytes,  // Comment: Passed to async task
}
```

### 1.2 Copy-on-Write (Cow)

**SHOULD**: When data is mostly read-only but occasionally needs escaping or normalization, use `Cow<'a, str>`.

**MUST**: Only incur heap allocation when modification is truly necessary.

**Example**:
```rust
use std::borrow::Cow;

// ✅ Cow optimization: allocate only when modification needed
fn clean<'a>(raw: &'a str) -> Cow<'a, str> {
    if raw.contains('\\') {
        Cow::Owned(unescape(raw))  // Allocate only for escaped strings
    } else {
        Cow::Borrowed(raw)  // Zero-copy for clean strings
    }
}

// Usage in parsing
fn parse_field(raw: &str) -> Cow<str> {
    clean(raw.trim())  // Chain operations without allocation
}
```

**When to Use Cow**:
- String escaping/unescaping
- Unicode normalization
- Trim operations (most strings already trimmed)
- Default value substitution

### 1.3 Cross-Async Passing: Bytes

**MUST**: When data blocks need to cross `.await` points or be shared between multiple tasks, use `bytes::Bytes` instead of `Vec<u8>` for O(1) reference-counted slice sharing.

**Prohibited**: `Vec::clone()` on async boundaries.

**Example**:
```rust
use bytes::Bytes;

// ✅ Correct: Bytes for async boundaries
async fn process_packet(data: Bytes) {
    // Zero-copy clone: just increment refcount
    let clone = data.clone();
    
    tokio::spawn(async move {
        process(clone).await;
    });
    
    // Original data still usable
    process(data).await;
}

// ❌ Wrong: Vec clone on async boundary
async fn process_packet_wrong(data: Vec<u8>) {
    let clone = data.clone();  // Full heap allocation!
    // ...
}
```

**Why Bytes**:
- O(1) clone: just increment atomic refcount
- Zero-copy slicing: `bytes.slice(range)` shares underlying buffer
- Immutable: safe to share across tasks

---

## 2. Arena Architecture: Bulk Allocation, Instant Reclamation

### 2.1 When to Enable

**SHOULD**: When parsing大量 short-lived, interconnected small objects (e.g., AST trees, DPF node lists), adopt `bumpalo::Bump`.

**Activation Signal**: Profiler shows frequent small allocations in hot path, and objects share unified lifecycle.

**MUST**: Arena lifecycle must be **strictly bound** to single parse or request scope. **Prohibited** as global static variable.

**Example**:
```rust
use bumpalo::Bump;

fn parse_document(input: &[u8]) -> Result<Node> {
    let bump = Bump::new();
    
    // All allocations tied to this arena
    let root = bump.alloc(Node::Group(&[]));
    let child1 = bump.alloc(Node::Leaf(42));
    let child2 = bump.alloc(Node::Leaf(43));
    
    // Link nodes (all in same arena)
    *root = Node::Group(&[child1, child2]);
    
    // Process...
    let result = process(root);
    
    // When bump drops, ALL nodes reclaimed instantly - O(1) deallocation
    result
}
```

**Benefit**: Reduces thousands of `malloc`/`free` calls to single pointer movement — lock-free, no syscall overhead.

### 2.2 Safety Admonitions

**P0 Red Line**: **Prohibited** to return Arena-allocated objects to upper layers while Arena destructs first (dangling pointer). **Must guarantee** objects and Arena have same lifetime, or copy data.

**Correct Pattern**:
```rust
// ✅ Correct: Arena and objects share lifetime
fn parse_with_arena<'a>(bump: &'a Bump, input: &'a [u8]) -> &'a Node {
    let node = bump.alloc(Node::Leaf(42));
    node  // Node tied to Arena lifetime
}

// ❌ Wrong: Returning Arena-allocated object
fn parse_wrong(input: &[u8]) -> &Node {
    let bump = Bump::new();
    let node = bump.alloc(Node::Leaf(42));
    node  // ERROR: bump will drop, node becomes dangling
}
```

**Verification**: Use `cargo miri` to check for self-referential or dangling references.

```bash
# Run Miri to detect UB
cargo +nightly miri run
```

---

## 3. Stack-First & Capacity Pre-judgment

### 3.1 Stack Containers

**SHOULD**: For collections with明确 upper bound (≤ 16 or 32) and short lifecycle, use `arrayvec::ArrayVec`, `tinyvec::TinyVec`, or fixed-size arrays to avoid heap allocation.

**Example**:
```rust
use arrayvec::ArrayVec;

// ✅ Stack-allocated: no heap allocation for small collections
let mut cols: ArrayVec<&str, 16> = ArrayVec::new();
cols.push("col1");
cols.push("col2");
// ... up to 16 elements, all on stack

// ✅ Fixed-size array for known-small data
let mut buffer: [u8; 64] = [0; 64];
buffer[0..4].copy_from_slice(&header);
```

**When to Use**:
- CSV row parsing (typically < 32 columns)
- Protocol header fields
- Temporary accumulation in hot loops
- Intermediate results with known bounds

### 3.2 Explicit Pre-allocation

**MUST**: All `Vec`/`HashMap` must call `with_capacity()` when minimum capacity is known. **Prohibited** to use bare `push` in loops causing multiple reallocations.

**Good Example**:
```rust
// ✅ Pre-allocate with known or estimated capacity
let mut items = Vec::with_capacity(1000);
for i in 0..1000 {
    items.push(Item::new(i));  // No reallocation
}

// ✅ Reserve if capacity unknown but estimable
let mut items = Vec::new();
items.reserve(100);  // Pre-allocate to avoid early reallocations
for item in source {
    items.push(item);
}
```

**Bad Example**:
```rust
// ❌ Bad: Multiple reallocations in loop
let mut items = Vec::new();
for i in 0..1000 {
    items.push(Item::new(i));  // Reallocates ~10 times
}
```

**strict Mode**: Use `clippy::uninit_vec` and related lints to catch uninitialized access. Enable `dhat` for heap allocation monitoring.

---

## 4. Verification & Diagnostics

| Tool | Purpose | Mode Requirement |
|------|---------|------------------|
| `cargo miri` | Check zero-copy/Arena for UB | `strict` |
| `dhat / heaptrack` | Heap allocation analysis & leak detection | `strict` |
| `criterion` | Micro-benchmark to verify Arena replacement benefit | `strict` |
| `clippy (perf group)` | Catch unpreallocated patterns | `standard+` |

### Usage Examples

```bash
# 1. Miri for UB detection (especially zero-copy & Arena)
cargo +nightly miri run

# 2. dhat for heap profiling
cargo +nightly build -Z build-std
valgrind --tool=dhat ./target/debug/app

# 3. heaptrack for allocation patterns
heaptrack ./target/release/app
heaptrack_gui heaptrack.app.gz

# 4. Benchmark Arena vs standard allocation
cargo bench --bench arena_benchmark
```

---

## 5. Agent Memory Management Checklist

Before finalizing data architecture:

### Zero-Copy
- [ ] Are parsed fields returning references (`&str`/`&[u8]`) instead of `String` (unless necessary)?
- [ ] Is `Cow` used for escape/cleaning optimization?
- [ ] Is `Bytes` used for cross-async passing instead of `Vec<u8>`?

### Arena Allocation
- [ ] Is Arena lifecycle strictly bound to request scope?
- [ ] Are Arena-allocated objects not returned beyond Arena lifetime?
- [ ] Has Miri verified zero-copy safety (no UB)?

### Stack-First
- [ ] Are small collections (≤ 16-32 elements) using `ArrayVec` or fixed arrays?
- [ ] Are all `Vec::new()` changed to `Vec::with_capacity()` when size known?
- [ ] Is `reserve()` used for estimable capacities?

### Validation
- [ ] Has dhat/heaptrack analyzed heap patterns? (strict)
- [ ] Has criterion verified Arena benefit > 5%? (strict)
- [ ] Have clippy perf lints been checked? (standard+)

---

## 6. Integration with Overall Architecture

### 6.1 Design Philosophy Alignment

| Principle | Memory Management Application |
|-----------|------------------------------|
| **Economy of Motion** | Zero-copy: don't move, just point |
| **Physical Alignment** | Stack-first aligns with CPU cache |
| **Unity of Opposites** | Arena: bulk alloc, instant reclaim |

### 6.2 Priority Constraints

**P1 (Maintainability)** is primary beneficiary:
- Zero-copy improves predictability
- Stack allocation reduces mental overhead
- Clear ownership boundaries

**P3 (Performance)** is acceleration target:
- Arena for hot-path small objects
- Bytes for async boundaries
- Pre-allocation to avoid reallocations

### 6.3 Terminology Links

All specialized terms defined in [`glossary.md`](05-glossary.md):
- Zero-Copy
- Copy-on-Write (Cow)
- Arena Allocation
- Bump Allocation
- Stack vs Heap
- Capacity Pre-judgment

### 6.4 Related Documents

- [`00-mode-guide.md`](00-mode-guide.md) — Execution modes (rapid/standard/strict)
- [`01-priority-pyramid.md`](01-priority-pyramid.md) — P0-P3 priority framework
- [`11-concurrency.md`](11-concurrency.md) — Sharing data across threads (Arc, Bytes)
- [`25-performance-tuning.md`](25-performance-tuning.md) — Arena, cache locality, diagnostic tools
- [`26-advanced-testing.md`](26-advanced-testing.md) — Miri, dhat, heaptrack

---

## 7. Trade-offs & Decision Framework

### When to Use Zero-Copy

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| Parsing input buffers (DPF, JSON, protocol frames) | **Use borrowed references** | P1: Predictable, no allocation |
| Fields must outlive parser | **Use Arc/Bytes** | Necessity: Lifetime extension |
| Occasional string modification | **Use Cow** | P1: Allocate only when needed |
| Cross-async data passing | **Use Bytes** | P1: O(1) clone, safe sharing |

### When to Use Arena

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| AST trees, DPF node lists | **Use bumpalo** | P3: Bulk alloc, O(1) reclaim |
| Long-lived objects | **Use Box/Arc** | Necessity: Arena would leak |
| Objects cross thread boundaries | **Use Arc** | Necessity: Arena not thread-safe |
| Simple one-off allocations | **Use standard alloc** | P1: Simplicity over optimization |

### When to Use Stack Containers

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| Small fixed-size collections (≤ 16-32) | **Use ArrayVec** | P1: No heap, predictable |
| Unknown or large size | **Use Vec** | Necessity: Stack overflow risk |
| Growing beyond stack limits | **Use Vec with reserve** | P2: Avoid reallocation |

### Progressive Optimization Path

```
rapid (MVP)
├─ Use String, Vec, standard allocation
└─ Ignore zero-copy, Arena

↓ (Profiler identifies allocation bottleneck)

standard (Production)
├─ Apply zero-copy parsing
├─ Use Cow for optional modification
├─ Pre-allocate with capacity
└─ Document exceptions with comments

↓ (Extreme performance required)

strict (Ultra-high throughput)
├─ Arena for short-lived objects
├─ Bytes for async boundaries
├─ ArrayVec for small collections
├─ dhat + Miri verification
└─ Criterion benchmark reports
```

---

## 8. Related

### Within rust-architecture-guide
- [`00-mode-guide.md`](00-mode-guide.md) — Execution modes governing memory enforcement
- [`01-priority-pyramid.md`](01-priority-pyramid.md) — Priority framework (P0-P3)
- [`05-glossary.md`](05-glossary.md) — Memory management terminology
- [`11-concurrency.md`](11-concurrency.md) — Cross-thread sharing (Arc, Bytes)
- [`25-performance-tuning.md`](25-performance-tuning.md) — Arena, cache locality, diagnostics
- [`26-advanced-testing.md`](26-advanced-testing.md) — Miri, dhat, heaptrack

### External Resources
- [`bytes` crate documentation](https://docs.rs/bytes) — Bytes, Buf, BufMut traits
- [`bumpalo` crate documentation](https://docs.rs/bumpalo) — Fast bump allocation arena
- [`arrayvec` crate documentation](https://docs.rs/arrayvec) — Stack-allocated vectors
- [The Rust Programming Language — References and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html)

---

**Version History**: 
- v6.0 — Integrated priority pyramid, three-tier modes, Miri/dhat validation, upgraded to Agent-executable Skill
- v5.0 — Added zero-copy parsing, Cow optimization, Bytes for async
- v4.0 — Initial arena allocation guidelines
- v3.0 — Stack-first principle
- v2.0 — Capacity pre-judgment
- v1.0 — Initial data architecture specification
