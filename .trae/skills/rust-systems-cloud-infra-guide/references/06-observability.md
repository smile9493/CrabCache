---
title: "Observability & Testing"
description: "Structured logging, deterministic concurrency testing with loom/turmoil, and fault injection"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/16-observability.md", "rust-architecture-guide/references/26-advanced-testing.md"]
dependents: []
---

# Skill: Observability & Testing

## 👤 Profile

* **Domain**: All long-running services, distributed systems.
* **Environment**: Production with tracing aggregation (Jaeger, Datadog).
* **Philosophy**:
    * **Structured Logging**: Prohibit `println!`; every request must have `trace_id`.
    * **Deterministic Simulation**: Use `loom` / `turmoil` to simulate concurrent races and network partitions.

---

## ⚔️ Core Directives

### Action 1: Structured Logging Configuration
* **Scenario**: All service log output.
* **Red Line**: Prohibit `println!` / `eprintln!`. Must use `tracing` or `slog`.
* **Execution**:
    * Inject `trace_id`, `span_id`, `request_id` into every Span.
    * Production: JSON format; Development: pretty format.

### Action 2: Hot Path Silence
* **Scenario**: Log output in critical I/O paths.
* **Execution**: Only `TRACE` level logs or metrics.

### Action 3: Deterministic Concurrency Testing
* **Scenario**: Consensus protocol and concurrent data structure testing.
* **Execution**: Use `loom` for deterministic race condition detection.

### Action 4: Network Failure Simulation
* **Scenario**: Verify Raft protocol behavior under network partition.
* **Execution**: Use `turmoil` to simulate partition, delay, packet loss, node crash.

### Action 5: I/O Error Injection
* **Scenario**: Verify handling of disk full, `EINTR` etc.
* **Execution**: Mock file operations to return `io::Error`.

---

## 💻 Code Paradigms

### Paradigm A: tracing Initialization

```rust
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    fmt, EnvFilter,
};

fn init_logging() {
    let format = if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
        fmt::format().json().with_target(true).with_thread_ids(true)
    } else {
        fmt::format().pretty()
    };
    
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().event_format(format))
        .init();
}
```

### Paradigm B: Span Injection

```rust
use tracing::{info, span, Instrument, Level};

async fn handle_request(req: Request) -> Response {
    let span = span!(Level::INFO, "request", 
        trace_id = %req.trace_id(),
        request_id = %req.id(),
    );
    
    async move {
        info!("Processing request");
        let result = process(req).await;
        info!("Request completed");
        result
    }.instrument(span).await
}
```

### Paradigm C: loom Concurrency Testing

```rust
#[cfg(test)]
mod tests {
    use loom::model::Model;
    
    #[test]
    fn test_concurrent_queue() {
        Model::new().run(|| {
            let q = Arc::new(LockFreeQueue::new());
            
            let q1 = q.clone();
            let h1 = loom::thread::spawn(move || {
                q1.push(1);
            });
            
            let q2 = q.clone();
            let h2 = loom::thread::spawn(move || {
                q2.push(2);
            });
            
            h1.join().unwrap();
            h2.join().unwrap();
        });
    }
}
```

### Paradigm D: turmoil Network Failure Simulation

```rust
use turmoil::{Builder, Result};

#[test]
fn test_raft_under_partition() -> Result<()> {
    let mut sim = Builder::new().build();
    
    sim.client("leader", |host| {
        host.run(async { /* Start leader */ })
    });
    
    sim.client("follower", |host| {
        host.run(async { /* Start follower */ })
    });
    
    // Simulate network partition
    sim.partition("leader", "follower");
    
    // Verify system behavior
    sim.step()?;
    
    Ok(())
}
```

### Paradigm E: I/O Error Injection

```rust
use mockall::automock;

#[automock]
trait FileSystem {
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
}

#[test]
fn test_disk_full_handling() {
    let mut fs = MockFileSystem::new();
    fs.expect_write()
        .returning(|_, _| Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "disk full"
        )));
    
    let engine = StorageEngine::new(Box::new(fs));
    let result = engine.write(&key, &value);
    
    assert!(result.is_err());
}
```
