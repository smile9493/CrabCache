---
title: "Concurrency & Lock Architecture"
description: "Lock spectrum, decision trees, async isolation, and poisoning handling for Rust concurrent programming"
category: "Architecture"
priority: "P0-P3"
applies_to: ["rapid", "standard", "strict"]
prerequisites: []
dependents: ["rust-systems-cloud-infra-guide/references/07-lock-free.md"]
---

# Concurrency & Lock Architecture

> **📋 Document Profile**
> - **Domain**: All Rust concurrent programming scenarios
> - **Priority**: P0 (safety) → P1 (maintainability) → P3 (performance)
> - **Modes**: `rapid` (basic) → `standard` (full rules) → `strict` (formal verification)
> - **Prerequisites**: None (foundational document)
> - **Deepened by**: [`07-lock-free.md`](../../rust-systems-cloud-infra-guide/references/07-lock-free.md) (cloud infrastructure scenarios)

## 1. Core Principles (Aligned with Priority Pyramid)

- **P0 - Safety**: Guarantee data race freedom; thread safety verified by the type system.
- **P1 - Maintainability**: Prioritize clear, reasoning-friendly concurrency models; avoid complex locks.
- **P2 - Compile Time**: Avoid excessive generics or macros causing build bloat (does not affect locks themselves).
- **P3 - Performance**: Introduce lock-free optimizations only after profiler confirms lock contention is the bottleneck.

## 2. Lock Classification and Selection Guide

### 2.1 Lock Spectrum

| Lock Type | Use Case | Typical Rust Implementation | Overhead & Risks |
|-----------|----------|----------------------------|------------------|
| Mutex | General shared mutable state; critical section should not be long-running | `std::sync::Mutex` | Context switch overhead; watch for poisoning |
| RwLock | Read-heavy, write-light; reads don't modify internal state | `std::sync::RwLock` | Writer starvation risk; poisoning also applies |
| Spinlock | Extremely short lock hold time (few CPU instructions); threads won't sleep/block in critical section | `spin::Mutex` or manual `AtomicBool` | Wastes CPU; not suitable for user-space long tasks |
| SeqLock | Write-rare, read-frequent; protected data is compact (e.g., few integers) | Manual or `seqlock` crate | Readers may retry; writers need atomic update |
| Optimistic Lock (CAS) | Low-conflict update operations, e.g., counters, lock-free stacks | `AtomicU64` + `compare_exchange` | Must handle ABA problem and memory ordering |
| RCU / Epoch | Read-extremely-frequent, write-rare; tolerates delayed reclamation | `crossbeam::epoch` | Memory reclamation timing is complex; requires global epoch |

### 2.2 Decision Tree (Top-Down Priority)

1. Is shared mutable state really necessary?
   - No → Use message passing (Channel/Actor) → **End**
   - Yes → Continue
2. Can mutable state be isolated within a single task or thread? → Yes → Actor pattern
3. Read-heavy, write-light? → Yes → Prefer `RwLock` (but watch for writer starvation)
4. Is lock hold time extremely short (nanoseconds)? → Yes → Spinlock
5. Does read/write pattern satisfy SeqLock conditions? → Yes → Consider SeqLock
6. Otherwise → Use `Mutex`

## 3. Strong Constraints for Rust Concurrent Programming

### 3.1 Compile-Time Data Race Immunity

- **Rule**: Rust does not allow passing non-`Send` types across threads or performing mutable borrows without using synchronization primitives. Shared mutable data across threads **MUST** be wrapped in `Arc<Mutex<T>>` or `Arc<RwLock<T>>`.
- **Forbidden**: Using `unsafe` to bypass these restrictions unless there is rigorous proof and encapsulation.

### 3.2 Minimal Critical Section

- **MUST**: Lock guard lifetime must cover only indispensable shared state access; release immediately afterward.
- **Implementation**: Use block expressions or `drop(guard)` to explicitly release early.
- **Forbidden**: Executing I/O, async operations, expensive computations, or locking other locks within critical sections (unless following lock ordering).

**Positive Example**:

```rust
let value = {
    let guard = mutex.lock().unwrap();
    guard.data.clone()  // Assuming data is small and cloneable
}; // Lock released
process(value);
```

**Negative Example**:

```rust
let guard = mutex.lock().unwrap();
// Forbidden: database query or .await while holding lock
db_query(...);
```

### 3.3 Async Lock Isolation

**Red Line**: Absolutely forbidden to call `.await` while holding `std::sync::Mutex` or `std::sync::RwLock` guards. This not only triggers compiler warnings (clippy::await_holding_lock) but may also block executor threads, causing implicit deadlocks.

**Alternatives**:

- If state must be held across `.await`, MUST use async locks (e.g., `tokio::sync::Mutex`). However, must evaluate whether refactoring to state machine or independent task is possible before use.
- Use `tokio::task::spawn_blocking` to migrate synchronous critical sections to blocking thread pool, avoiding occupation of async worker threads.

**Example**:

```rust
// ✅ Correct: async lock across await
let mut guard = async_mutex.lock().await;
async_action().await; // Safe, but watch for deadlock risk
drop(guard);

// ❌ Wrong: sync lock across await
let guard = sync_mutex.lock().unwrap();
async_action().await; // Will trigger compiler warning; may cause deadlock/starvation at runtime
```

### 3.4 Lock Poisoning Handling

**MUST**: Every `lock()` call must explicitly match on `Result`; forbidden to directly `unwrap()` unless Poison is proven impossible (e.g., simple numeric increment, and threads previously holding lock cannot panic).

**Strategy Classification**:

- **Fail-Fast**: When lock protects core invariants (e.g., B-Tree structure, consistent hashing ring), poisoned state cannot guarantee internal consistency; MUST log error and immediately `panic!` or abort process, relying on monitoring system to restart.

- **Controlled Recovery**: If lock protects disposable or rebuildable soft state (e.g., cache, connection pool), after capturing `PoisonError`, MUST completely clear/reinitialize internal data, then call `into_inner()` to acquire lock ownership. Forbidden to directly use dirty data from before poisoning.

**Example**:

```rust
let mut cache = match cache_lock.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
        warn!("Cache lock poisoned, rebuilding");
        let mut inner = poisoned.into_inner();
        *inner = Cache::new();  // Complete rebuild
        inner
    }
};
```

## 4. Evolution Toward Lock-Free Architecture

### 4.1 Architectural Lock Elimination

- **SHOULD** Prefer Actor model + bounded channels to encapsulate shared mutable state within independent tasks.
- **SHOULD** Adopt RCU pattern (e.g., `crossbeam::epoch`) for high-concurrency read-only data structures, achieving lock-free reads.
- **MUST NOT** Use global `RwLock<HashMap<K,V>>` becoming a bottleneck; instead adopt sharded locks (e.g., `dashmap::DashMap`) or concurrent hash tables.

### 4.2 Atomic Operations and Memory Ordering

- When using lock-free structures, MUST explicitly choose appropriate memory ordering; forbidden to blindly use `SeqCst` (unless truly necessary).
- **MUST** Use standard library atomic types or primitives provided by crossbeam; do not write manual inline assembly implementations.
- **SHOULD** Use loom or miri to verify correctness of lock-free code.

## 5. Engineering Assurance and Testing

### 5.1 Deadlock Prevention

- **MUST** Specify global lock acquisition order and document it in code comments.
- **SHOULD** Use no_deadlocks crate or loom runtime model for deadlock checking.

### 5.2 Concurrency Correctness Testing

- **MUST** Include multi-threaded stress testing in CI to reproduce potential races.
- **SHOULD** Use loom for exhaustive testing of core synchronization logic.
- **MAY** Use cargo fuzz to fuzz test state machine consistency.

### 5.3 Performance Validation

**P3 Performance Optimization Prerequisite**: Any lock-free replacement or lock optimization MUST include profiling reports (e.g., perf, cachegrind output) proving original lock is the bottleneck.

## 6. Glossary

- **Critical Section**: Code segment protected by lock.
- **Lock Poisoning**: When thread holding lock panics, lock is marked as unusable state.
- **SeqLock**: Lightweight lock where writers don't block readers via sequence numbers.
- **Memory Ordering (Ordering)**: Defines visibility ordering guarantees of atomic operations to other threads.

## 7. Integration with Overall Architecture Standards

This specification is part of the Concurrency specialty within Rust Coding Standards Skills体系; must coordinate with following rules:

- [data-architecture.md](09-data-architecture.md): Prioritize Owned passing; reduce sharing.
- [error-handling.md](10-error-handling.md): Lock poisoning handling is part of P0 error handling.
- [async-internals.md](12-async-internals.md): Detailed considerations for async locks.

## 8. Practical Examples

### 8.1 Message Passing Priority (Actor Model)

```rust
use tokio::sync::mpsc;

// ✅ Preferred: Actor model + MPSC channels
let (tx, rx) = mpsc::channel(100); // Bounded channel for backpressure
```

**Backpressure Requirement**:

- **Must use bounded channels** to prevent out-of-memory errors
- Bounded channels apply backpressure automatically
- Unbounded channels **only** when consumption >> production is strictly guaranteed

### 8.2 Mutex Strategies

#### std::sync::Mutex (Synchronous - Minimal Critical Sections)

```rust
// ✅ Minimal critical section
{
    let guard = mutex.lock().unwrap();
    let value = guard.some_field; // Extract scalar or clone needed data
    drop(guard); // Release immediately via drop() or block scope
    // Expensive computation OUTSIDE lock
}

// ❌ Forbidden: await while holding sync lock
// This blocks the thread pool and can cause deadlocks
```

**Compromise**: For non-cloneable large resources, execute necessary operations inside lock, but avoid time-consuming computations.

#### tokio::sync::Mutex (Async)

```rust
// ✅ Only for indivisible "async transactions"
use tokio::sync::Mutex;

async fn transfer(account: Arc<Mutex<Account>>, amount: u64) {
    let mut guard = account.lock().await;
    guard.balance -= amount; // Allowed to await inside
}
```

### 8.3 RwLock Strategy

Use `RwLock` when access pattern is **read-heavy** (>80% reads):

```rust
use std::sync::RwLock;

let cache: RwLock<HashMap<String, Data>> = RwLock::new(HashMap::new());

// ✅ Multiple readers can hold read lock simultaneously
let data = cache.read().unwrap().get("key").cloned();

// ✅ Write lock is exclusive — only when mutation needed
cache.write().unwrap().insert("key".into(), new_data);
```

**Decision Matrix: Mutex vs RwLock**

| Scenario | Choice | Reason |
|----------|--------|--------|
| Read-heavy (>80% reads) | `RwLock` | Concurrent reads, no contention |
| Write-heavy or balanced | `Mutex` | `RwLock` write overhead > `Mutex` |
| Tiny critical section | `Mutex` | Simpler, lower overhead |
| Read-heavy + high contention | `DashMap` | Sharded, lock-free reads |

### 8.4 parking_lot Alternative

For production systems, prefer `parking_lot` over `std::sync`:

```toml
[dependencies]
parking_lot = "0.12"
```

| Feature | `std::sync` | `parking_lot` |
|---------|------------|---------------|
| Lock size | 40 bytes (Mutex) | 1 byte (Mutex) |
| Poisoning | Yes (panic on lock) | No (faster, no panic) |
| Fairness | Not guaranteed | Fair FIFO |
| Performance | Good | Better (smaller, faster) |

```rust
// ✅ parking_lot: smaller, faster, no poisoning
use parking_lot::Mutex;
let guard = mutex.lock(); // No .unwrap() needed — no poisoning
```

### 8.5 Deadlock Prevention

**Rules**:

1. **Never hold more than one lock at a time** — if you must, always acquire in the same order
2. **Use `std::sync::Mutex` with timeout** — `lock().unwrap()` can deadlock; consider `try_lock` with retry
3. **Prefer channels over multiple locks** — eliminates deadlock by construction
4. **Test with `loom`** — model check all concurrent code paths

```rust
// ❌ Deadlock: two locks acquired in different order
fn transfer(a: &Mutex<Account>, b: &Mutex<Account>) {
    let mut ga = a.lock().unwrap();
    let mut gb = b.lock().unwrap(); // May deadlock if another thread does transfer(b, a)
}

// ✅ Channel-based: deadlock-free by design
enum Command {
    Transfer { from: Id, to: Id, amount: u64 },
}
// Single actor processes commands sequentially — no deadlock possible
```

### 8.6 Synchronization Primitive Selection

| Primitive | Use When | Example |
|-----------|----------|---------|
| `Barrier` | Wait for N threads to reach a point | Parallel initialization |
| `Once` / `OnceLock` | One-time initialization | Global config, logger setup |
| `Condvar` | Wait + signal pattern | Producer-consumer without channels |
| `Atomic` | Lock-free counters/flags | Metrics, sequence numbers |

```rust
use std::sync::atomic::{AtomicU64, Ordering};

// ✅ Lock-free counter with precise memory ordering
static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

fn handle_request() {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed); // No synchronization needed
}
```

## 9. Trade-offs

- **Message passing vs Shared state**: Channels eliminate data races but add communication overhead.
- **Sync vs Async locks**: Sync locks are faster but can't cross await points. Async locks are flexible but slower.
- **Mutex vs RwLock**: RwLock enables concurrent reads but has higher write overhead. Use only when reads >> writes.
- **std::sync vs parking_lot**: parking_lot is smaller and faster but removes poisoning (a safety net for panic-during-lock).

## Related

- [09-data-architecture.md](09-data-architecture.md) — Ownership in concurrent contexts
- [10-error-handling.md](10-error-handling.md) — Lock poisoning handling as P0 error
- [12-async-internals.md](12-async-internals.md) — Async lock boundaries
- [25-performance-tuning.md](25-performance-tuning.md) — Lock-free concurrency and atomics

---

**Version History**: v2.0 — Integrated architecture specification; added SeqLock, atomic operations, and testing guidelines.
