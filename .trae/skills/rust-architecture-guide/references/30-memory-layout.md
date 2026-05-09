---
title: "Memory Layout Transparency"
description: "Struct padding audit, repr(C) mandate, cache-friendly design, alignment control"
category: "Architecture"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["09-data-architecture.md"]
dependents: ["31-breakwater-pattern.md"]
---

# Memory Layout Transparency

> **Philosophy**: From "ideological safety" to "materialist physics". Rust's `Safe` guarantees logical boundary safety, but only physical layout control (`#[repr(C)]`, alignment, packing) guarantees **performance inevitability**.

---

## 1. The Materialist Foundation: Padding is Waste

Every byte of struct padding is dead weight — it consumes cache lines, inflates memory footprint, and degrades throughput.

### Padding Audit Example

```rust
// ❌ 24 bytes — 14 bytes wasted as padding (58% waste!)
struct BadLayout {
    flag: u8,      // 1 byte + 7 padding
    name: String,  // 24 bytes (ptr + len + cap)
    count: u32,    // 4 bytes + 4 padding
}

// ✅ 16 bytes — only 3 bytes padding (19% waste)
struct GoodLayout {
    name: String,  // 24 bytes → but reordered
    count: u32,    // 4 bytes
    flag: u8,      // 1 byte
}
```

### Hard Constraint: Field Ordering by Alignment Descending

**Rule**: Sort struct fields from largest alignment to smallest. This minimizes padding waste automatically.

```rust
// Ordering rule: String (align 8) > u64 (align 8) > u32 (align 4) > u16 (align 2) > u8 (align 1)
struct OptimizedLayout {
    big_field: String,    // align 8
    counter: u64,         // align 8
    index: u32,           // align 4
    flags: u16,           // align 2
    tag: u8,              // align 1
    // 1 byte trailing padding (unavoidable for array alignment)
}
```

---

## 2. `#[repr(C)]` Mandate (MUST)

All structs crossing FFI/WASM boundaries or persisted to disk **must** be explicitly marked with `#[repr(C)]`.

```rust
// MUST for FFI/WASM/disk
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetworkHeader {
    pub version: u8,
    pub flags: u16,
    pub payload_len: u32,
    pub checksum: u32,
}

// MUST for disk serialization
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DiskRecord {
    pub id: u64,
    pub offset: u64,
    pub length: u32,
}
```

**Rationale**: Rust's default `#[repr(Rust)]` has no guaranteed layout order. Cross-boundary or persistent data requires deterministic byte layout.

---

## 3. Compile-Time Size & Alignment Assertions (MUST)

Use `static_assertions` to enforce layout constraints at compile time.

```rust
use static_assertions::{assert_eq_size, assert_eq_align};

#[repr(C)]
pub struct CacheLineAligned<T> {
    pub data: T,
    _pad: [u8; 64 - std::mem::size_of::<T>() % 64],
}

assert_eq_size!(CacheLineAligned<u64>, [u8; 64]);
assert_eq_align!(CacheLineAligned<u64>, 64);

// Enforce max struct size for hot-path data
assert!(std::mem::size_of::<NetworkHeader>() <= 16);
```

---

## 4. Cache-Friendly Design: Hot-Path ≤ 64 Bytes (SHOULD)

**Rule**: Key hot-path data structures should fit within a single cache line (64 bytes) to avoid cache line splits and false sharing.

```rust
// Hot-path counter — must be cache-line aligned
#[repr(C, align(64))]
pub struct HotCounter {
    pub value: u64,
    // padded to 64 bytes automatically by align(64)
}
```

### False Sharing Prevention

When multiple threads access adjacent data on different cache lines, CPU cache coherency causes severe performance degradation.

```rust
// ❌ False sharing: two counters on same cache line
struct SharedCounters {
    pub a: u64,
    pub b: u64,  // Thread 2 invalidates Thread 1's cache line
}

// ✅ Cache-line separated
struct SeparatedCounters {
    #[repr(C, align(64))]
    pub a: CacheAligned<u64>,
    #[repr(C, align(64))]
    pub b: CacheAligned<u64>,
}
```

---

## 5. Alignment Control Patterns

### Pattern A: Explicit Alignment for SIMD

```rust
#[repr(C, align(32))]
pub struct SimdBuffer {
    pub data: [f32; 8],  // 32 bytes, aligned to 32
}

// SAFETY: alignment guaranteed by repr(align(32))
pub unsafe fn process_simd(buf: &SimdBuffer) {
    let ptr = buf.data.as_ptr();
    // _mm256_load_ps requires 32-byte alignment
}
```

### Pattern B: Packed for Wire Format

```rust
#[repr(C, packed)]
pub struct WirePacket {
    pub opcode: u8,
    pub length: u16,  // no padding between u8 and u16
    pub data: [u8; 8],
}

// Access packed fields via unaligned read
impl WirePacket {
    pub fn length(&self) -> u16 {
        unsafe { ptr::read_unaligned(&self.length as *const u16) }
    }
}
```

### Pattern C: Cache-Line Isolation for Multi-Threaded State

```rust
use std::cell::UnsafeCell;

#[repr(C, align(64))]
pub struct ThreadLocalState {
    pub counter: UnsafeCell<u64>,
    pub flags: UnsafeCell<u32>,
    _pad: [u8; 56],  // ensure total is exactly 64 bytes
}
```

---

## 6. Mandatory CI Checks

```toml
# Cargo.toml — compile-time size enforcement
[dependencies]
static_assertions = "1"

[lints.clippy]
large_enum_variant = "deny"    # variants with huge size differences
box_collection = "deny"        # boxed collections are cache-hostile
```

```rust
// lib.rs — deny large stack frames
#![deny(clippy::large_stack_frames)]

// Compile-time size guard for public API
#[cfg(test)]
mod size_checks {
    use super::*;
    use static_assertions::const_assert;

    const_assert!(std::mem::size_of::<PublicRequest>() <= 128);
    const_assert!(std::mem::size_of::<HotState>() <= 64);
}
```

---

## 7. Layout Decision Matrix

| Scenario | Requirement | Action |
|----------|------------|--------|
| Cross FFI/WASM | Deterministic layout | `#[repr(C)]` |
| Persisted to disk | Stable byte order | `#[repr(C)]` + explicit endianness |
| SIMD operations | Specific alignment | `#[repr(C, align(N))]` |
| Wire protocol | No padding | `#[repr(C, packed)]` |
| Hot path data | Cache-friendly | ≤ 64 bytes, sort by alignment desc |
| Multi-threaded shared | No false sharing | `#[repr(C, align(64))]` per thread |
| General struct | Minimal waste | Sort fields by alignment descending |
