---
title: "Graceful Degradation & Resilience Engineering"
description: "Graceful shutdown, tokio-graceful-shutdown, JoinSet structured concurrency, lock poisoning recovery, health checks, and circuit breakers"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["02-backpressure.md"]
dependents: []
aligned_with: ["Tokio Graceful Shutdown Guide", "tokio-graceful-shutdown crate", "async-shutdown crate"]
---

# Skill: Graceful Degradation & Resilience Engineering

## 👤 Profile

* **Domain**: Cloud-native services, long-running daemons, K8s deployments.
* **Environment**: Container orchestration with restart policies.
* **Philosophy**:
    * **Graceful Degradation**: Infrastructure must have self-protection, graceful exit, and partial recovery capabilities.
    * **Lock Poisoning Recovery**: Explicitly handle `PoisonError`, choose Fail-Fast or Recovery.
    * **Structured Concurrency**: Tasks as first-class lifecycle entities — spawn, monitor, cancel as a group.

---

## ⚔️ Core Directives

### Action 1: Graceful Shutdown Flow
* **Scenario**: Receive `SIGTERM` or `SIGINT` signal.
* **Red Line**: Prohibit `kill -9` brutal termination.
* **Execution Flow**:
    1. Capture signal → Trigger `CancellationToken`.
    2. Stop accepting new requests.
    3. Wait for in-flight tasks (with timeout).
    4. Fsync WAL / persist state.
    5. Release kernel resources (file locks).
    6. Exit.

### Action 2: Lock Poisoning Handling Strategy
* **Scenario**: `Mutex` / `RwLock` poisoned when holding thread panics.
* **Red Line**: Must explicitly handle `LockResult::Err`, prohibit direct `.unwrap()`.
* **Strategy Choice**:
    * **Fail-Fast**: Critical metadata corrupted → `panic!()` to trigger restart.
    * **Recovery**: Loose cache → `into_inner()` clear and rebuild.

### Action 3: Health Check Endpoints
* **Scenario**: K8s probes or load balancer health checks.
* **Required Endpoints**: `/health`, `/ready`, `/metrics`.
* **Required Metrics**: Queue capacity, active tasks, memory usage, page faults.

---

## 💻 Code Paradigms

### Paradigm A: Graceful Shutdown Implementation

```rust
use tokio_util::sync::CancellationToken;
use tokio::task::JoinSet;

struct GracefulShutdown {
    cancel_token: CancellationToken,
    tasks: JoinSet<()>,
    drain_timeout: Duration,
}

impl GracefulShutdown {
    async fn shutdown(&mut self) {
        info!("Received shutdown signal");
        
        // 1. Trigger cancellation
        self.cancel_token.cancel();
        
        // 2. Stop accepting new requests
        // (check cancel_token in listener)
        
        // 3. Wait for existing tasks
        match tokio::time::timeout(
            self.drain_timeout,
            self.tasks.join_all()
        ).await {
            Ok(_) => info!("All tasks completed"),
            Err(_) => warn!("Timeout, forcing exit"),
        }
        
        // 4. Flush persistent state
        self.flush_state().await;
        
        // 5. Exit
        std::process::exit(0);
    }
}
```

### Paradigm B: Fail-Fast Lock Poisoning

```rust
use std::sync::{Mutex, MutexGuard};

struct CriticalMetadata {
    btree_root: Mutex<BTreeNode>,
}

impl CriticalMetadata {
    fn access(&self) -> MutexGuard<BTreeNode> {
        match self.btree_root.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // Data structure corrupted, cannot recover
                error!("B-Tree corrupted, triggering restart");
                panic!("Critical metadata corrupted");
            }
        }
    }
}
```

### Paradigm C: Recovery Lock Poisoning

```rust
struct Cache {
    data: Mutex<LruCache<Key, Value>>,
}

impl Cache {
    fn get(&self, key: &Key) -> Option<Value> {
        match self.data.lock() {
            Ok(mut guard) => guard.get(key).cloned(),
            Err(poisoned) => {
                warn!("Cache poisoned, rebuilding");
                let mut guard = poisoned.into_inner();
                guard.clear();
                guard.rebuild();
                None
            }
        }
    }
}
```

### Paradigm D: Health Check Endpoints

```rust
use axum::{extract::State, Json};
use serde_json::json;

struct HealthChecker {
    queue_metrics: QueueMetrics,
    task_metrics: TaskMetrics,
    memory_metrics: MemoryMetrics,
}

async fn health() -> &'static str {
    "OK"
}

async fn ready(State(checker): State<HealthChecker>) -> Json<serde_json::Value> {
    Json(json!({
        "queues": {
            "current": checker.queue_metrics.current(),
            "max": checker.queue_metrics.max(),
            "rejects": checker.queue_metrics.rejects(),
        },
        "tasks": {
            "active": checker.task_metrics.active(),
            "pending": checker.task_metrics.pending(),
        },
        "memory": {
            "used": checker.memory_metrics.used(),
            "page_faults": checker.memory_metrics.page_faults(),
        },
    }))
}
```

---

## 📊 Failure Degradation Matrix

| Failure Type | Detection | Degradation Strategy |
|--------------|-----------|---------------------|
| Queue full | Length >= threshold | Drop new or block upstream |
| Connection pool exhausted | Idle = 0 | Reject or wait |
| Memory pressure | > 90% | GC or evict cache |
| Downstream unavailable | Consecutive failures | Circuit break |
| Disk full | Usage > 95% | Read-only mode |
