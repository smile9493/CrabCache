# Lock-free Concurrency & Memory Reclamation

> **📚 Prerequisites**: This document assumes mastery of basic lock spectrum and decision tree from [`11-concurrency.md`](../../rust-architecture-guide/references/11-concurrency.md) §4 (Atomic Operations and Memory Ordering).
> 
> **🔺 Deepening Direction**: Hardening general lock strategies for 128+ cores, 10GbE scenarios, introducing RCU and lock-free structures with formal verification.
> 
> **📋 Document Profile**:
> - **Domain**: 10GbE NIC packet routing, database global metadata trees, HFT order books
> - **Environment**: 128+ core CPU, extremely high concurrent reads, ultra-low latency requirements
> - **Mode**: `standard` (awareness) → `strict` (mandatory implementation)
> - **Prerequisites**: [`11-concurrency.md`](../../rust-architecture-guide/references/11-concurrency.md), [`25-performance-tuning.md`](../../rust-architecture-guide/references/25-performance-tuning.md)

## Philosophy

* **Dialectical Materialism (唯物辩证法, Space-Time Trade-offs)**: Acknowledge read and write as inherent contradictions. Resolve concurrency conflicts through spatial copying (RCU), and resolve old object destruction through temporal partitioning (Epoch).
* **Jeet Kune Do (截拳道, Zero-Blocking Flow)**: Read operations must "flow like water" — absolute zero blocking, bypassing all mutex obstacles.

---

## Core Directives

### Action 1: RCU Pattern (Read-Copy-Update) — Flow Like Water Past Obstacles

* **Scenario**: Global routing tables or config trees with read-heavy, write-sparse access patterns.
* **Red Line**: In such scenarios, **absolutely prohibit** `std::sync::RwLock` or `Mutex`. At 100+ core scale, even `RwLock` read locks cause severe performance collapse due to cache line contention.
* **Execution**:
    1. Use `Arc::make_mut` or dedicated RCU library (`arc-swap`) for absolute zero-blocking reads.
    2. Writer strategy: Clone a data snapshot (Copy), modify in invisible copy (Update), publish via atomic pointer swap (Swap).
    3. Readers flow like water past writer boulders, achieving perfect spatial decoupling of read-write contradiction.

### Action 2: Epoch-based Memory Reclamation — Temporal Dialectics of Creation and Destruction

* **Scenario**: Building complex lock-free concurrent structures (Lock-free Queue, Lock-free SkipList). The biggest pain point is ABA problem and dangling pointers.
* **Red Line**: Strictly prohibit directly calling `drop` on shared atomic pointers without concurrent protection.
* **Execution**:
    1. Introduce `crossbeam-epoch` or Hazard Pointers for safe memory reclamation during multi-threaded lock-free reads.
    2. Establish "Epochs" on concurrent timeline. Acknowledge that old objects (expired memory) require a destruction process — physically reclaim only after ensuring all observer threads have crossed the old epoch.

### Action 3: Memory Ordering Precision — Causality at Physical Level

* **Scenario**: When using `Atomic` variables for lock-free state transitions.
* **Red Line**: **Prohibit** blind use of `Ordering::SeqCst` (global sequential consistency). It triggers extremely expensive memory barriers at CPU level, forcibly synchronizing all cores' L1 caches.
* **Execution**:
    1. Precisely analyze memory visibility based on causal relationships.
    2. Most publish-subscribe patterns only need `Ordering::Release` (publish data) paired with `Ordering::Acquire` (acquire data).
    3. For statistical counters without logical dependencies, enforce `Ordering::Relaxed`.

---

## Code Paradigms

### Paradigm A: RCU Global Routing Table Zero-Blocking Update (via `arc-swap`)

```rust
use arc_swap::ArcSwap;
use std::sync::Arc;
use std::collections::HashMap;

pub struct GlobalRouter {
    routes: ArcSwap<HashMap<String, String>>,
}

impl GlobalRouter {
    pub fn new(initial: HashMap<String, String>) -> Self {
        Self { routes: ArcSwap::from_pointee(initial) }
    }

    pub fn lookup(&self, key: &str) -> Option<String> {
        let guard = self.routes.load();
        guard.get(key).cloned()
    }

    pub fn update_route(&self, key: String, dest: String) {
        let current_arc = self.routes.load();
        let mut new_routes = (*current_arc).clone();
        new_routes.insert(key, dest);
        self.routes.store(Arc::new(new_routes));
    }
}
```

### Paradigm B: Epoch-based Reclamation for Lock-free Node Deletion (via `crossbeam-epoch`)

```rust
use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};
use std::sync::atomic::Ordering::{Acquire, Release};

struct Node<T> {
    data: T,
    next: Atomic<Node<T>>,
}

pub struct LockFreeStack<T> {
    head: Atomic<Node<T>>,
}

impl<T> LockFreeStack<T> {
    pub fn pop(&self) -> Option<T> {
        let guard = &epoch::pin();

        loop {
            let head_shared = self.head.load(Acquire, guard);

            if head_shared.is_null() {
                return None;
            }

            // SAFETY: We hold guard and checked non-null, dangling pointers cannot occur here
            let head_ref = unsafe { head_shared.as_ref().unwrap() };
            let next_shared = head_ref.next.load(Acquire, guard);

            if self.head.compare_exchange(head_shared, next_shared, Release, Acquire, guard).is_ok() {
                // Temporal dialectics: Cannot immediately drop popped node,
                // other reader threads may still be observing it.
                // Defer to epoch garbage collector for physical reclamation
                // after all reader threads cross this epoch.
                unsafe {
                    guard.defer_destroy(head_shared);
                }

                return Some(unsafe { std::ptr::read(&head_ref.data) });
            }
        }
    }

    pub fn push(&self, data: T) {
        let guard = &epoch::pin();
        let new_node = Owned::new(Node { data, next: Atomic::null() }).into_shared(guard);

        loop {
            let head = self.head.load(Acquire, guard);
            unsafe { (*new_node.as_ptr()).next.store(head, Release) };

            if self.head.compare_exchange(head, new_node, Release, Acquire, guard).is_ok() {
                return;
            }
        }
    }
}
```

### Paradigm C: Memory Ordering Precision

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

struct Publisher {
    ready: AtomicBool,
    data: AtomicU64,
}

impl Publisher {
    pub fn publish(&self, value: u64) {
        self.data.store(value, Ordering::Relaxed);
        self.ready.store(true, Ordering::Release);
    }
}

struct Subscriber {
    ready: AtomicBool,
    data: AtomicU64,
}

impl Subscriber {
    pub fn check(&self) -> Option<u64> {
        if self.ready.load(Ordering::Acquire) {
            Some(self.data.load(Ordering::Relaxed))
        } else {
            None
        }
    }
}
```

---

## Prohibitions Quick List

| Category | Prohibited | Mandatory |
|----------|------------|-----------|
| High-read scenario | `std::sync::RwLock` / `Mutex` | `arc-swap` (zero-block read) |
| Lock-free reclamation | Direct `drop` on shared atomic pointer | `crossbeam-epoch` Guard / Hazard Pointers |
| Memory ordering | Blind `Ordering::SeqCst` | `Release` + `Acquire` pair; `Relaxed` for counters |
| ABA problem | Unprotected CAS operations | Epoch / Hazard Pointers protection |

---

## Dependencies

```toml
[dependencies]
arc-swap = "1"
crossbeam-epoch = "0.11"
```
