# Rust Systems & Cloud Infrastructure Guide

[![Version](https://img.shields.io/badge/Version-v6.0.0-orange.svg)]()
[![Reference Docs](https://img.shields.io/badge/Reference-13%20Docs-blue.svg)]()
[![CI Lints](https://img.shields.io/badge/CI%20Lints-11%20Strict-red.svg)]()

**Cloud-native infrastructure Rust guide** — Vertical deepening for database kernels, distributed storage, high-performance gateways, container runtimes, eBPF control planes, and OS components targeting long-running systems.

English | [简体中文](README.zh-CN.md)

---

## Overview

This guide is a **vertical deepening supplement** to [`rust-architecture-guide`](../rust-architecture-guide/), targeting:

- Database kernels & storage engines
- Distributed message queues
- High-performance gateways / proxies
- Container runtimes, eBPF control planes
- OS components

**Environment assumptions**: Long-running nodes (uptime > 1 year), 10GbE+ networks, multi-NUMA architectures.

## Core Philosophy

| Principle | Description |
|-----------|-------------|
| **Mechanical Sympathy** | Software design aligned with hardware physical characteristics (CPU cache, NUMA, PMEM, kernel I/O stack) |
| **Determinism** | Eliminate non-determinism (time, randomness, HashMap ordering), ensure reproducible state machines |
| **Resilience** | Graceful degradation over crash, backpressure over OOM, structured concurrency over leaks |
| **Jeet Kune Do** | One-strike memory lifecycle (Arena), flow like water adapting to hardware channels (Allocator API) |

### Mechanical Sympathy — Align with Physics

Ultimate performance comes not from clever tricks, but from deep resonance between code logic and underlying physical hardware. Software runs not on abstract machines, but on:

```
L1 Cache (32KB, 4 cycles) → L2 (256KB, 12 cycles) → L3 (shared, 40 cycles) → DRAM (200+ cycles)
NUMA Node 0 ← QPI/UPI → NUMA Node 1
NIC Ring Buffer → Kernel TCP Stack → User Space
```

Performance is not optimized — it is **aligned**. When you understand cache lines are 64 bytes, false sharing destroys concurrency, and `mmap` page faults cost microseconds, you stop "optimizing" and start **designing** structures that resonate with hardware.

### Determinism — Eliminate Non-determinism

Distributed consensus requires bit-level reproducibility. If two nodes receive the same input, their output must be bit-identical. Any non-determinism is poison to consensus — it creates forks in Raft logs, and forks are the deepest fear of distributed systems.

**Prohibited in consensus logic**: `Instant::now()`, `rand::random()`, `HashMap` iteration order.

### Resilience — Absorb, Don't Resist

Systems inevitably trend toward entropy (failures, OOM, network partitions). The resilient response:
- **Don't resist** — absorb (backpressure over OOM)
- **Don't crash** — degrade (graceful degradation over crash)
- **Don't die** — rebirth (K8s restart with persisted state)

### Jeet Kune Do — One Strike, Flow Like Water

- **One Strike**: Arena allocation — allocate once, reclaim in bulk, O(1) lifecycle management
- **Flow Like Water**: Allocator API — data flows to NUMA-local nodes, flows to PMEM persistent layer, adapting to hardware channels

## Hard Rules (Absolute Prohibitions)

| Category | Prohibit | Enforce |
|----------|----------|---------|
| **Channels** | `std::sync::mpsc::channel()` unbounded | `tokio::sync::mpsc::channel(LIMIT)` |
| **Cancellation** | Direct non-idempotent writes in `select!` | `spawn` + `oneshot` |
| **Time** | `Instant::now()` in state machine Apply | Timestamp proposed by Leader |
| **Shutdown** | Immediate exit on `SIGTERM` | Graceful shutdown (cancel token → wait inflight → fsync → exit) |
| **FFI** | `extern` functions without `catch_unwind` | Catch panic + return error code |
| **Per-request allocation** | AST nodes on global heap | Arena (`bumpalo`) |
| **Arena escape** | Saving Arena pointer to external static variable | Clone to global heap when needed |
| **Nightly allocator** | `std::alloc::Allocator` in production | `allocator_api2` (stable) |
| **Fragmented allocation** | Direct `alloc::alloc` at FFI boundary | Pre-allocated Slab (`mmap` + `mlock`) |
| **Allocation panic** | `panic!` on allocation failure | `Result<T, AllocError>` + backpressure |
| **Read-heavy path locks** | `RwLock`/`Mutex` on 100+ core read paths | `arc-swap` (RCU zero-blocking read) |
| **Lock-free reclamation** | Unprotected `drop` of shared pointers | `crossbeam-epoch` Guard |
| **Memory ordering** | Blind `Ordering::SeqCst` | `Release`+`Acquire` pairs; `Relaxed` for counters |
| **Byte-by-byte loops** | Core parse loops with `if byte == b'\n'` | SIMD bitmask batch comparison |
| **AoS aggregation** | Array of Structs for bulk aggregation | SoA (Struct of Arrays) columnar layout |

---

## Document Index

`references/` directory contains **13 in-depth reference documents** covering core cloud infrastructure domains:

### 1. I/O & Zero-Copy

| # | Document | Coverage |
|---|----------|----------|
| **01** | [io-model.md](references/01-io-model.md) | I/O model selection: Tokio epoll vs io_uring vs monoio decision tree + zero-copy pipeline (`splice`/`sendfile`/`copy_file_range` via `rustix`) + `bytes::Bytes` O(1) clone + Direct I/O (`O_DIRECT` + 512B/4KB alignment) + mixed runtime red line |

### 2. Backpressure & Cancellation Safety

| # | Document | Coverage |
|---|----------|----------|
| **02** | [backpressure.md](references/02-backpressure.md) | Bounded channels + `Semaphore` global concurrency limit + HTTP 503 `Retry-After` propagation + cancellation safety (non-idempotent writes: `spawn` + `oneshot`) + circuit breaker |

### 3. Syscalls & eBPF

| # | Document | Coverage |
|---|----------|----------|
| **03** | [syscall.md](references/03-syscall.md) | Wrapper priority (`rustix` → `nix` → `libc`) + eBPF integration (`aya`/`libbpf-rs` userspace control plane + BPF verifier kernel safety) + error code mapping (EINTR auto-retry / EAGAIN wait or return / ENOMEM trigger degradation / EINVAL immediate return) |

### 4. Consensus & Determinism

| # | Document | Coverage |
|---|----------|----------|
| **04** | [consensus.md](references/04-consensus.md) | Raft/Paxos Apply absolute determinism (prohibit `Instant::now()`/`rand`/`HashMap` ordering) + `BTreeMap`/`IndexMap` alternatives + state fingerprint verification (SHA-256 cross-node consistency) + zero-copy serialization (`rkyv`/`FlatBuffers` vs `serde_json`/`bincode`) |

### 5. Resilience Design

| # | Document | Coverage |
|---|----------|----------|
| **05** | [resilience.md](references/05-resilience.md) | Graceful shutdown flow (`SIGTERM` → CancellationToken → stop accepting → wait in-flight → fsync WAL → release resources → exit) + Lock Poisoning handling (Fail-Fast vs Recovery strategy) + K8s health check endpoints + failure degradation matrix |

### 6. Observability

| # | Document | Coverage |
|---|----------|----------|
| **06** | [observability.md](references/06-observability.md) | Structured logging (`tracing_subscriber` JSON/pretty + `trace_id`/`span_id` injection) + hot path silence (TRACE/Metrics only) + `loom` deterministic concurrency testing + `turmoil` network fault simulation + I/O error injection |

### 7. Lock-Free Concurrency

| # | Document | Coverage |
|---|----------|----------|
| **07** | [lock-free.md](references/07-lock-free.md) | RCU pattern (`arc-swap` zero-blocking read + Read-Copy-Update spatial decoupling) + Epoch reclamation (`crossbeam-epoch` Guard + `defer_destroy` temporal partitioning + ABA problem defense) + memory ordering precision (Release+Acquire pairs / Relaxed counters / prohibit blind `SeqCst`) |

### 8. Vectorized Execution

| # | Document | Coverage |
|---|----------|----------|
| **08** | [vectorized.md](references/08-vectorized.md) | SIMD instructions (`std::simd` portable / `core::arch::x86_64` AVX-512) + Bitmask branch elimination + SoA columnar layout + LLVM auto-vectorization + `as_simd::<32>()` batch processing |

### 9. Code Style

| # | Document | Coverage |
|---|----------|----------|
| **09** | [code-style.md](references/09-code-style.md) | Capacity awareness (`Vec::with_capacity`) + hot path zero-allocation (`arrayvec`/`itoa` over `format!`) + SAFETY comment enforcement + FFI `catch_unwind` + prohibit Mutex across `.await` + exhaustive pattern matching + narrow critical section pattern |

### 10. CI Lints

| # | Document | Coverage |
|---|----------|----------|
| **10** | [ci-lints.md](references/10-ci-lints.md) | 11 deny-level lints (`await_holding_lock`/`unwrap_used`/`expect_used`/`undocumented_unsafe_blocks`/`large_stack_frames`/`todo`/`dbg_macro`/`unimplemented`/`unsafe_op_in_unsafe_fn`/`non_ascii_idents`) + test environment `cfg_attr` relaxation + Cargo.toml `[lints]` config |

### 11. Advanced Memory

| # | Document | Coverage |
|---|----------|----------|
| **11** | [memory-advanced.md](references/11-memory-advanced.md) | Arena (`bumpalo` per-request lifecycle + O(1) bulk reclamation + prohibit Arena pointer escape) + Slab pre-allocation (`mmap` + `mlock` + `crossbeam_queue::ArrayQueue` O(1) lock-free allocation) + Allocator API (`allocator_api2` stable + NUMA-aware + PMEM mapping) + memory exhaustion backpressure (`Result<T, AllocError>` + 503 rejection + LRU eviction + prohibit `panic!`) |

### 12. Breakwater Pattern

| # | Document | Coverage |
|---|----------|----------|
| **12** | [breakwater-pattern.md](references/12-breakwater-pattern.md) | Facade/Core layered architecture + Facade responsibilities (Rate Limiting, Circuit Breaker, Input Validation, Backpressure, Graceful Degradation) + Core invariants (No Network I/O, No Locks, Pure Logic, Zero-Copy) + Boundary Interception Protocol (type-contracted, validated, O(1) conversion) + de-oxygenation protocol (`RawRequest` → `ValidatedCommand`) |

### 13. Physical Feasibility Audit

| # | Document | Coverage |
|---|----------|----------|
| **13** | [physical-audit.md](references/13-physical-audit.md) | Mandatory pre-design audit + Container memory limit audit (70% threshold → circuit breaker) + Network latency budget (p50/p99 distribution analysis + I/O > 30% CPU time → force batching) + NUMA topology audit (cross-node penalty ~2x → lock-free redesign if contention > 20%) + Cloud audit report template |

---

## Relationship

```
rust-architecture-guide (Universal Constitution)
          │
          └──► rust-systems-cloud-infra-guide (Vertical Deepening)
                      │
                      ├── I/O Model: Tokio vs io_uring
                      ├── Backpressure: bounded channels + Semaphore
                      ├── Syscalls: rustix wrappers
                      ├── Consensus: deterministic state machines
                      ├── Resilience: graceful shutdown + circuit breaker
                      ├── Observability: tracing + metrics
                      ├── Lock-free: RCU + Epoch + memory ordering
                      ├── Vectorized: SIMD + SoA
                      └── Memory: Arena + Slab + Allocator API
```

- This guide depends on `rust-architecture-guide`'s P0-P3 priority framework and execution modes
- This guide adds system-level red lines and hardware alignment constraints on top of P0 safety for cloud-native scenarios
- **Complementary use**: The universal constitution is the foundation; this guide is the cloud infrastructure-specific amendment

---

## File Structure

```
rust-systems-cloud-infra-guide/
├── SKILL.md                          # Skill entry (Agent instructions)
├── README.md                         # This file — document index
└── references/                        # 13 in-depth reference documents
    ├── 01-io-model.md               # I/O model & zero-copy
    ├── 02-backpressure.md           # Backpressure & cancellation safety
    ├── 03-syscall.md                # Syscall wrappers
    ├── 04-consensus.md              # Consensus & determinism
    ├── 05-resilience.md             # Resilience design
    ├── 06-observability.md          # Observability
    ├── 07-lock-free.md              # Lock-free concurrency
    ├── 08-vectorized.md             # Vectorized execution
    ├── 09-code-style.md             # Code style
    ├── 10-ci-lints.md               # CI lints
    ├── 11-memory-advanced.md        # Advanced memory
    ├── 12-breakwater-pattern.md     # Facade/Core layered architecture
    └── 13-physical-audit.md         # Physical feasibility audit
```

---

## License

[MIT](../LICENSE)
