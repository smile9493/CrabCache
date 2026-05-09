---
title: "Vectorized Execution & Columnar Memory Layout"
description: "SIMD vectorization, SoA columnar layout, and auto-vectorization for compute-bound hot paths"
category: "Infrastructure"
priority: "P3"
applies_to: ["strict"]
prerequisites: ["rust-architecture-guide/references/09-data-architecture.md", "rust-architecture-guide/references/25-performance-tuning.md"]
dependents: []
---

# Vectorized Execution & Columnar Memory Layout

> **📚 Prerequisites**: This document assumes understanding of basic data layout principles from [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md) §3 (Cache Affinity & Data Layout) and [`25-performance-tuning.md`](../../rust-architecture-guide/references/25-performance-tuning.md) §4 (SIMD Vectorization).
> 
> **🔺 Deepening Direction**: Applying SIMD auto-vectorization and manual intrinsics to 10GbE+ networking, database executors, and HFT data processing with hardware-aligned memory layouts.
> 
> **📋 Document Profile**:
> - **Domain**: Database vectorized execution engines (e.g., Databend), 10GbE NIC protocol parsing, high-frequency financial data cleaning
> - **Environment**: Modern multi-core CPU, I/O bypassed via io_uring or DPDK, computation is the sole bottleneck
> - **Mode**: `strict` (mandatory for compute-bound hot paths)
> - **Prerequisites**: [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md), [`25-performance-tuning.md`](../../rust-architecture-guide/references/25-performance-tuning.md)

## Philosophy

* **Dialectical Materialism (唯物辩证法, Hardware Sympathy)**: Acknowledge CPU cache hit rate as inviolable physical law. Abandon human-intuitive object arrays (AoS), adopt machine-intuitive columnar layout (SoA).
* **Jeet Kune Do (截拳道, One-Strike Optimization)**: Discard byte-by-byte "fancy but useless" processing. Combine compressed time and space actions, using SIMD instructions to crush multiple data in one clock cycle, achieving GB/s throughput.

---

## Core Directives

### Action 1: SIMD Vectorized Interception — The Strongest Strike Across Clock Cycles

* **Scenario**: JSON extraction at gateway layer, protocol header parsing, or bulk memory search.
* **Red Line**: In core parsing `for` loops, **prohibit** byte-by-byte comparison (e.g., `if byte == b'\n'`). This wastes compute power and causes severe branch misprediction.
* **Execution**:
    1. No longer process byte-by-byte in loops.
    2. Use `std::simd` (Portable SIMD) or `core::arch::x86_64` (e.g., AVX-512) to crush 16, 32, or even 64 data elements in one CPU clock cycle.
    3. Convert control flow to data flow, using Bitmask to eliminate all `if-else` branches.

### Action 2: Columnar Memory Layout — Physical Arrangement Conforming to Hardware

* **Scenario**: Building database executors or batch computation components processing massive structured data.
* **Red Line**: **Prohibit** Array of Structs (AoS) for large-scale aggregation computation scenarios.
* **Execution**:
    1. Organize data in compact columnar format (e.g., Apache Arrow standard).
    2. Conform to CPU prefetching objective laws — when computing a column, ensure L1 Cache is preloaded with only valid data from that column, never mixing in useless adjacent fields.
    3. Combine Rust iterators for auto-vectorization, making code structure fully conform to silicon hardware thinking.

---

## Code Paradigms

### Paradigm A: Columnar Layout & Auto-vectorization (SoA)

```rust
// ❌ AoS (Array of Structs) — violates physical laws
// CPU Cache Line fills with unnecessary padding and redundant fields
struct TradeAoS { price: f64, volume: f64, timestamp: u64 }
let trades: Vec<TradeAoS> = load_trades();
// Auto-vectorization easily fails because price data is not contiguous in memory

// ✅ SoA (Struct of Arrays) — conforms to physical laws
struct TradeSoA {
    prices: Vec<f64>,
    volumes: Vec<f64>,
    timestamps: Vec<u64>,
}

impl TradeSoA {
    #[inline(never)]
    pub fn calculate_total_value(&self) -> f64 {
        // Data absolutely contiguous in memory, CPU hardware prefetcher at peak efficiency
        self.prices.iter()
            .zip(self.volumes.iter())
            .map(|(&p, &v)| p * v)
            .sum()
    }
}
```

### Paradigm B: Explicit SIMD Dimensionality Reduction (via `std::simd`)

```rust
#![feature(portable_simd)]
use std::simd::cmp::SimdPartialEq;
use std::simd::u8x32;

pub fn fast_find_newline(data: &[u8]) -> Option<usize> {
    let (prefix, chunks, suffix) = data.as_simd::<32>();

    // 1. Handle unaligned prefix (scalar fallback)
    if let Some(pos) = prefix.iter().position(|&b| b == b'\n') {
        return Some(pos);
    }

    // 2. SIMD core strike: crush 32 bytes at once
    let target = u8x32::splat(b'\n');
    for (i, chunk) in chunks.iter().enumerate() {
        let mask = chunk.simd_eq(target);
        if mask.any() {
            return Some(prefix.len() + i * 32 + mask.first_set().unwrap());
        }
    }

    // 3. Handle suffix (scalar fallback)
    suffix.iter().position(|&b| b == b'\n').map(|p| prefix.len() + chunks.len() * 32 + p)
}
```

### Paradigm C: AVX-512 Explicit Intrinsics (High-performance Scenarios)

```rust
use core::arch::x86_64::*;

#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_sum_f32(data: &[f32]) -> f32 {
    let mut sum = _mm512_setzero_ps();
    let mut i = 0;

    // Process 16 f32 at once (512-bit = 16 * 32-bit)
    while i + 16 <= data.len() {
        let chunk = _mm512_loadu_ps(&data[i]);
        sum = _mm512_add_ps(sum, chunk);
        i += 16;
    }

    // Horizontal add 16 results
    let result = _mm512_reduce_add_ps(sum);

    // Handle remainder
    let mut remainder = result;
    while i < data.len() {
        remainder += data[i];
        i += 1;
    }
    remainder
}
```

---

## Prohibitions Quick List

| Category | Prohibited | Mandatory |
|----------|------------|-----------|
| Byte-by-byte loop | Byte-by-byte `if byte == b'\n'` | SIMD bitmask bulk comparison |
| Columnar aggregation | AoS (Array of Structs) | SoA (Struct of Arrays), columnar layout |
| Branching in hot path | Branch misprediction-prone `if-else` | Data flow replacing control flow, Bitmask merging |
| Unaligned SIMD | Unaligned `as_simd` | Ensure data alignment or use `as_simd_unaligned` |

---

## Feature Gates

```toml
# Cargo.toml
[features]
nightly = ["dep:std_simd"]

[dependencies]
portable_simd = { package = "std_simd", version = "0.1", optional = true }
```

```rust
// Use nightly portable SIMD (Rust 1.80+ stable std::simd)
#![feature(portable_simd)]

// Or use target-platform-specific AVX-512
#[target_feature(enable = "avx512f")]
unsafe fn hot_path() { /* ... */ }
```
