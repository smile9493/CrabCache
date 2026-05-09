---
title: "Backpressure & Resource Control"
description: "Bounded channels, graceful degradation, and backpressure propagation in K8s environments"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/11-concurrency.md"]
dependents: ["05-resilience.md"]
---

# Skill: Backpressure & Resource Control

## 👤 Profile

* **Domain**: Cloud-native services, API gateways, distributed systems.
* **Environment**: K8s Cgroups with strict memory/CPU limits.
* **Philosophy**:
    * **Resource Determinism**: OOM is a P0 incident; tail latency must be controllable.
    * **Backpressure Propagation**: Pressure should propagate upstream, not accumulate internally.

---

## ⚔️ Core Directives

### Action 1: Mandatory Bounded Channels
* **Scenario**: All producer-consumer models, connection pools, async task dispatch.
* **Red Line**: **Absolutely prohibit** unbounded channels.
* **Execution**:
    * Use `tokio::sync::mpsc::channel(LIMIT)`.
    * Producer handles `SendError` (retry, drop, or block).

### Action 2: Hard Concurrency Limit
* **Scenario**: Gateway ingress preventing excessive requests from flooding internal systems.
* **Execution**: Use `tokio::sync::Semaphore` for global concurrent request limit.

### Action 3: Backpressure Propagation Strategy
* **Scenario**: Local queue is full.
* **Execution**:
    * Upstream: Return HTTP 503 with `Retry-After` header.
    * Internal: Degrade priority or shed load.

### Action 4: Cancellation Safety Defense
* **Scenario**: Listen to multiple Futures in `select!`, client may cancel at any time.
* **Red Line**: All participating Futures must be cancellation-safe.
* **Unsafe Operations**:
    * Modify global state
    * Write to file without `sync_all`
    * Send but unconfirmed messages
    * Non-idempotent RPC
* **Safe Pattern**: `spawn` independent task + `oneshot` to pass result back.

---

## 💻 Code Paradigms

### Paradigm A: Bounded Channel

```rust
use tokio::sync::mpsc;

const MAX_QUEUE_SIZE: usize = 10_000;

async fn bounded_channel_example() {
    let (tx, mut rx) = mpsc::channel(MAX_QUEUE_SIZE);
    
    tokio::spawn(async move {
        while let Some(item) = rx.recv().await {
            process(item).await;
        }
    });
    
    match tx.send(data).await {
        Ok(_) => {}
        Err(_) => handle_closed_receiver(),
    }
}
```

### Paradigm B: Backpressure Controller

```rust
use tokio::sync::{Semaphore, mpsc};
use std::sync::Arc;

struct BackpressureController {
    semaphore: Arc<Semaphore>,
    queue_tx: mpsc::Sender<Request>,
}

impl BackpressureController {
    async fn handle_request(&self, req: Request) -> Result<Response, Error> {
        let permit = self.semaphore.try_acquire()
            .map_err(|_| Error::TooManyRequests)?;
        
        self.queue_tx.send(req).await
            .map_err(|_| Error::QueueFull)?;
        
        drop(permit);
        Ok(Response::Accepted)
    }
}
```

### Paradigm C: Cancellation-Safe Pattern

```rust
use tokio::sync::oneshot;

async fn safe_io_with_cancel(
    data: Vec<u8>,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Option<Vec<u8>> {
    let (tx, rx) = oneshot::channel();
    
    tokio::spawn(async move {
        let result = perform_io(data).await;
        let _ = tx.send(result);
    });
    
    tokio::select! {
        result = rx => result.ok(),
        _ = cancel.changed() => None,
    }
}
```

### Paradigm D: Circuit Breaker

```rust
use std::sync::atomic::{AtomicU64, Ordering};

struct CircuitBreaker {
    failure_count: AtomicU64,
    last_failure: AtomicU64,
    threshold: u64,
    cooldown_ms: u64,
}

impl CircuitBreaker {
    fn should_allow(&self) -> bool {
        let failures = self.failure_count.load(Ordering::Relaxed);
        if failures < self.threshold {
            return true;
        }
        let last = self.last_failure.load(Ordering::Relaxed);
        current_time_ms() - last > self.cooldown_ms
    }
}
```
