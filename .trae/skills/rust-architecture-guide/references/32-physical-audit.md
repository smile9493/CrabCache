---
title: "Physical Feasibility Audit Specification"
description: "Mandatory audit between project initiation and design blueprint: I/O budget, memory ceiling, concurrency true cost"
category: "Architecture"
priority: "P0"
applies_to: ["standard", "strict"]
prerequisites: ["03-project-lifecycle.md"]
dependents: []
---

# Physical Feasibility Audit Specification

> **Philosophy**: The Jeet Kune Do interception punch — predict failure before the design blueprint is drawn. Measure the distance to the opponent before throwing the fist.

---

## 1. Audit Timing

**When**: After project initiation (立项), before design blueprint (架构图) is finalized.

**Who**: Lead engineer + team review.

**Output**: A **Physical Boundary Audit Report** (物理边界审计报告) — a living document referenced throughout the project lifecycle.

---

## 2. The Three Audit Elements

### 2.1 I/O Budget Assessment

**Question**: Can the cross-boundary communication frequency support the response time target?

**Red Line**: If estimated I/O overhead exceeds 30% of CPU processing time, **batching is mandatory**.

**Audit Steps**:

1. **Identify all boundaries**: FFI calls, network requests, disk I/O, WASM bridge crossings.
2. **Estimate per-call cost**:
   - JS → WASM FFI: ~100ns per call
   - Network round-trip (local): ~1ms
   - Disk read (SSD): ~100μs
   - Disk read (HDD): ~5ms
3. **Calculate total budget**:
   ```
   Target latency: 16.67ms (60fps) / 100ms (web) / 10ms (HFT)
   I/O budget = total calls × per-call cost
   CPU budget = target latency - I/O budget
   ```
4. **Apply 30% rule**: If I/O budget > 30% of target latency → force batching.

**Example Audit**:

| Operation | Count | Cost Each | Total |
|-----------|-------|-----------|-------|
| JS → WASM per-element | 10,000 | 100ns | 1ms |
| JS → WASM per-attribute | 100,000 | 100ns | 10ms ← **30% threshold exceeded** |
| Network fetch | 1 | 50ms | 50ms |
| **Total** | | | **61ms** |

**Verdict**: Per-attribute FFI calls must be batched. Single-call batch reduces 100,000 calls to 1 call → 100ns saved.

---

### 2.2 Memory Ceiling Assessment

**Question**: In the target environment (WASM / container / embedded), where is the hard RSS growth inflection point?

**Audit Steps**:

1. **Identify target environment limits**:
   - WASM linear memory: typically 256MB–2GB (grows only)
   - Container: cgroup memory limit (e.g., 512MB)
   - Embedded: physical RAM (e.g., 64MB)
2. **Estimate steady-state memory**:
   - Hot data structures: `size_of` × count
   - Buffer pools: pre-allocated size
   - Arena overhead: bump allocation waste (~10%)
3. **Calculate growth trajectory**:
   ```
   Baseline (startup): X MB
   Steady-state (normal load): Y MB
   Peak (stress load): Z MB
   Limit (environment): L MB
   Safety margin: (L - Z) / L
   ```
4. **Red Line**: If safety margin < 20%, redesign with bounded queues or backpressure.

**Example Audit**:

| Component | Size | Count | Total |
|-----------|------|-------|-------|
| Session state | 2KB | 100,000 | 200MB |
| Request arena | 64KB | 100 concurrent | 6.4MB |
| Buffer pool | 1MB | 50 | 50MB |
| **Total steady-state** | | | **256.4MB** |
| WASM limit | | | **512MB** |
| **Safety margin** | | | **50%** ✅ |

---

### 2.3 Concurrency True Cost Assessment

**Question**: Under multi-core contention, does the cache invalidation time from atomic operations exceed the computation time?

**Audit Steps**:

1. **Identify all shared state**: Atomics, Mutexes, RwLocks, channels.
2. **Estimate contention cost**:
   - Atomic load (no contention): ~1ns
   - Atomic load (contended): ~50-200ns (cache line bounce)
   - Mutex lock (no contention): ~25ns
   - Mutex lock (contended): ~1-10μs (kernel syscall)
3. **Calculate contention ratio**:
   ```
   contention_cost = atomic_ops_per_tick × avg_contention_latency
   computation_cost = cpu_cycles / instructions_per_cycle
   contention_ratio = contention_cost / (contention_cost + computation_cost)
   ```
4. **Red Line**: If contention_ratio > 20%, redesign with lock-free structures or thread-local caching.

**Example Audit**:

| Shared State | Access Rate | Contention Latency | Total |
|-------------|-------------|-------------------|-------|
| Global counter (AtomicU64) | 1M ops/s | 100ns | 100ms/s |
| Request cache (Mutex) | 100K ops/s | 1μs | 100ms/s |
| **Total contention** | | | **200ms/s** |
| **Computation** | | | **800ms/s** |
| **Contention ratio** | | | **20%** ⚠️ (threshold) |

**Verdict**: Global counter should use thread-local batching — each thread accumulates locally, flushes periodically. Reduces contention by ~90%.

---

## 3. Audit Report Template

```markdown
# Physical Boundary Audit Report: [Project Name]

## Environment
- Target: WASM / Container / Bare Metal
- Memory limit: [X MB]
- Latency target: [Y ms]
- Concurrency: [Z threads]

## I/O Budget
| Boundary | Call Rate | Per-Call Cost | Total | Within Budget? |
|----------|-----------|---------------|-------|----------------|
| ... | ... | ... | ... | ... |

## Memory Ceiling
| Component | Size | Count | Total | Notes |
|-----------|------|-------|-------|-------|
| ... | ... | ... | ... | ... |
| **Safety Margin** | | | **X%** | ≥ 20% required |

## Concurrency True Cost
| Shared State | Contention Cost | Computation Cost | Ratio | Action |
|-------------|----------------|-----------------|-------|--------|
| ... | ... | ... | ... | ... |

## Verdict
- [ ] PASS: All metrics within thresholds
- [ ] FAIL: Redesign required (specify which metric)

## Required Actions
1. [Action item]
2. [Action item]
```

---

## 4. Red Line Values

| Metric | Threshold | Required Action |
|--------|-----------|----------------|
| I/O > 30% of CPU time | Force batching | Redesign to reduce call frequency |
| Memory safety margin < 20% | Add backpressure | Bounded queues, LRU eviction |
| Contention ratio > 20% | Lock-free redesign | Thread-local caching, epoch-based GC |
| WASM initial load > 100KB | Code splitting | Dynamic `import()` for lazy loading |
| Per-frame allocation > 1MB | Arena required | Pre-allocate or bump allocator |

---

## 5. Philosophy: The Interception Punch

This audit is the **Jeet Kune Do interception** — it strikes problems before they manifest in code. By calculating physical boundaries at the design stage, we avoid the fatal flaw of building systems that "work in development" but "fail under load."

**The principle**: Measure before you move. Every architecture decision must survive the physical feasibility audit before it becomes code.
