---
title: "Breakwater Pattern for Cloud Services"
description: "Facade/Core layered architecture for long-running systems: high-tolerance external API, zero-overload internal kernel"
category: "Architecture"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/31-breakwater-pattern.md"]
dependents: []
---

# Breakwater Pattern for Cloud Services

> **Prerequisites**: This document extends the generic breakwater pattern from [`rust-architecture-guide/references/31-breakwater-pattern.md`](../../rust-architecture-guide/references/31-breakwater-pattern.md) for long-running cloud systems.

> **Philosophy**: Hardware Sympathy + Resilience. The Facade absorbs client chaos with graceful degradation; the Core processes with absolute determinism.

---

## 1. Cloud-Specific Adaptation

In cloud environments, the breakwater pattern must address:
- **Network latency variability** (p99 latency can be 10x p50)
- **Partial failures** (downstream service degradation)
- **Load spikes** (traffic bursts from autoscaling lag)

### Architecture

```
                    Client Requests (Chaos)
                            │
                            ▼
              ┌─────────────────────────────┐
              │    Facade (虚)               │
              │  Rate limiting, validation,  │
              │  circuit breaker, fallback   │
              ├─────────────────────────────┤
              │  Boundary Interception       │
              │  Type contraction, O(1)      │
              │  validation, backpressure    │
              ├─────────────────────────────┤
              │    Core (实)                 │
              │  Deterministic processing,   │
              │  zero-copy, no std::sync     │
              │  Mutex across .await          │
              └─────────────────────────────┘
```

---

## 2. Facade Responsibilities (虚)

The Facade layer absorbs all external chaos:

1. **Rate Limiting**: Token bucket or sliding window before Core sees the request
2. **Circuit Breaker**: Open circuit when downstream error rate exceeds threshold
3. **Input Validation**: Reject malformed requests before they reach Core
4. **Backpressure**: Bounded queue with rejection when full
5. **Graceful Degradation**: Return cached/stale data when Core is overloaded

```rust
// Facade: high-level service entry point
pub struct ServiceFacade {
    rate_limiter: TokenBucket,
    circuit_breaker: CircuitBreaker,
    request_queue: BoundedChannel<ValidatedRequest>,
}

impl ServiceFacade {
    pub async fn handle(&self, raw: IncomingRequest) -> Result<Response, ServiceError> {
        // Step 1: Rate limit
        if !self.rate_limiter.try_acquire() {
            return Err(ServiceError::TooManyRequests);
        }

        // Step 2: Circuit breaker
        if self.circuit_breaker.is_open() {
            return Err(ServiceError::ServiceUnavailable);
        }

        // Step 3: Validation + type contraction (de-oxygenation)
        let validated = raw.validate().map_err(ServiceError::BadRequest)?;

        // Step 4: Queue with backpressure
        self.request_queue
            .send(validated)
            .await
            .map_err(|_| ServiceError::Overloaded)?;

        // Step 5: Core processing
        Ok(process_request(&validated).await?)
    }
}
```

---

## 3. Core Responsibilities (实)

The Core layer processes with absolute determinism:

1. **No Network I/O**: All external calls happen in Facade
2. **No Locks**: Thread-local or lock-free data structures
3. **Pure Logic**: Given validated input, produce deterministic output
4. **Zero-Copy**: Operate on `&[u8]` or `Bytes` views, not owned `Vec`

```rust
// Core: pure, deterministic processing
pub async fn process_request(req: &ValidatedRequest) -> Result<CoreResponse, ProcessingError> {
    // No external dependencies
    // All invariants guaranteed by Facade validation
    match req.operation {
        Operation::Query { key } => {
            let result = lookup_in_memory(key);  // Zero-copy
            Ok(CoreResponse::Found(result))
        }
        Operation::Update { key, value } => {
            apply_update(key, value);  // In-place mutation
            Ok(CoreResponse::Updated)
        }
    }
}
```

---

## 4. The Boundary Interception Protocol

**Mandatory**: All data crossing from Facade to Core must:
1. Be type-contracted (no `Option<T>` where T is always Some after validation)
2. Be validated (no invalid state can enter Core)
3. Be O(1) convertible (no deep copies)

```rust
// Facade input: rich, optional, potentially invalid
pub struct IncomingRequest {
    pub id: Option<String>,
    pub payload: Vec<u8>,
    pub timeout_ms: Option<u32>,
}

// Core input: contracted, mandatory, guaranteed valid
pub struct ValidatedRequest<'a> {
    pub id: NonEmptyString,    // validated: cannot be empty
    pub payload: &'a [u8],     // zero-copy view
    pub timeout: Duration,     // validated: within bounds
}

// The interception layer
impl<'a> TryFrom<&'a IncomingRequest> for ValidatedRequest<'a> {
    type Error = ValidationError;

    fn try_from(raw: &'a IncomingRequest) -> Result<Self, Self::Error> {
        let id = NonEmptyString::new(raw.id.as_deref().unwrap_or(""))
            .map_err(|_| ValidationError::EmptyId)?;

        let timeout = Duration::from_millis(raw.timeout_ms.unwrap_or(1000).min(30_000) as u64);

        Ok(Self {
            id,
            payload: &raw.payload,
            timeout,
        })
    }
}
```

---

## 5. Philosophy: The Harbor Wall

A harbor wall doesn't stop waves — it transforms their chaotic energy into calm water the harbor can handle. Similarly, the Facade doesn't reject all external chaos; it absorbs, validates, and transforms it into structured input the Core can process deterministically.

This is **Jeet Kune Do interception** applied to cloud services: catch errors at the boundary, before they propagate through the system. The Facade is the extended arm that absorbs impact; the Core is the strike that lands with certainty.
