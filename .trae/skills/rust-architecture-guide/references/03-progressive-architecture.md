---
title: "Progressive Architecture Framework"
description: "MVP to Production architecture evolution patterns"
category: "Architecture"
priority: "P1"
applies_to: ["rapid", "standard"]
prerequisites: ["01-priority-pyramid.md", "02-conflict-resolution.md"]
dependents: []
---

# Progressive Architecture Framework

## Philosophy

**Standards enable progress, not prevent it.**

Architecture should adapt to project lifecycle stages, not lock development speed.

## Lifecycle Stages

### Stage 1: MVP / Validation (Weeks 1-4)

**Goal**: Validate business hypothesis quickly

**Priorities**: P1 (understandable) > P2 (fast iteration) > P3 (performance)

**Allowed Compromises**:
- Single struct with enum + Option fields instead of state machines
- Runtime validation with `assert!` instead of type-level guarantees
- Simple error handling with `unwrap()` in controlled paths

### Stage 2: Product-Market Fit (Months 2-6)

**Goal**: Scale validated features

**Priorities**: P1 (maintainability) > P2 (compile time) > P3 (hot path perf)

**Refactoring Targets**:
- Core domain entities → Type-driven state machines
- Error handling → Structured error types
- Concurrency → Message passing over shared state

### Stage 3: Production Scale (6+ months)

**Goal**: Optimize proven bottlenecks

**Priorities**: P0 (safety) > P1 (clarity) > P3 (proven bottlenecks) > P2 (compile time)

**Optimizations**:
- Hot paths → Zero-copy, unsafe where profiled
- Cold paths → Keep simple, no premature optimization

---

## MVP Patterns

### Pattern 1: Enum + Option Instead of State Machine

**When**: Rapid prototyping, unproven business logic

```rust
// ✅ MVP: Flexible but runtime-checked
struct Order {
    id: OrderId,
    status: OrderStatus, // Enum: Created, Paid, Shipped
    
    // All possible state fields
    paid_at: Option<DateTime<Utc>>,
    payment_id: Option<PaymentId>,
    shipped_at: Option<DateTime<Utc>>,
    tracking_number: Option<String>,
}

impl Order {
    fn mark_paid(&mut self, payment_id: PaymentId) {
        assert!(self.status == OrderStatus::Created);
        self.status = OrderStatus::Paid;
        self.paid_at = Some(Utc::now());
        self.payment_id = Some(payment_id);
    }
}
```

**Tests Compensate**:
```rust
#[test]
fn test_order_lifecycle() {
    let mut order = Order::create();
    assert_eq!(order.status, OrderStatus::Created);
    
    order.mark_paid(PaymentId::new());
    assert_eq!(order.status, OrderStatus::Paid);
    assert!(order.paid_at.is_some());
}
```

### Pattern 2: Runtime Validation Over Type Guarantees

**When**: Business rules still evolving

```rust
// ✅ MVP: Runtime checks
impl Payment {
    fn process(&self) -> Result<()> {
        // Runtime validation
        if self.amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if self.recipient.is_empty() {
            return Err(Error::InvalidRecipient);
        }
        // ... more checks
        Ok(())
    }
}
```

### Pattern 3: Simple Error Handling

**When**: Internal tools, controlled environments

```rust
// ✅ MVP: Pragmatic error handling
fn load_config() -> Config {
    let content = std::fs::read_to_string("config.toml")
        .expect("Config file must exist"); // Controlled environment
    
    toml::from_str(&content)
        .expect("Config must be valid toml") // Validated in CI
}
```

---

## Production Patterns

### Pattern 1: Type-Driven State Machine

**When**: Core domain logic is stable and critical

```rust
// ✅ Production: Compile-time guarantees
enum OrderState {
    Created(OrderInfo),
    Paid(PaymentInfo),
    Shipped(ShipmentInfo),
    Completed,
}

impl OrderState {
    fn pay(self, payment: PaymentDetails) -> Result<Self, TransitionError> {
        match self {
            OrderState::Created(info) => {
                Ok(OrderState::Paid(PaymentInfo { info, payment }))
            }
            _ => Err(TransitionError::InvalidState),
        }
    }
    
    fn ship(self, tracking: String) -> Result<Self, TransitionError> {
        match self {
            OrderState::Paid(info) => {
                Ok(OrderState::Shipped(ShipmentInfo { info, tracking }))
            }
            _ => Err(TransitionError::InvalidState),
        }
    }
}

// Invalid transitions are compile errors
let order = OrderState::Created(info);
order.ship("tracking"); // ❌ Compile error!
```

### Pattern 2: Structured Error Types

**When**: Library or public API

```rust
// ✅ Production: Structured errors
#[derive(thiserror::Error, Debug)]
pub enum OrderError {
    #[error("Order not found: {0}")]
    NotFound(OrderId),
    
    #[error("Invalid transition: {from:?} → {to:?}")]
    InvalidTransition {
        from: OrderStatus,
        to: OrderStatus,
    },
    
    #[error("Payment failed: {0}")]
    PaymentFailed(#[from] PaymentError),
}
```

---

## Migration Strategies

### Compiler-Assisted Refactoring

**Why it works**: Rust compiler identifies all affected sites

**Process**:
1. Make breaking change (e.g., split enum into state machine)
2. Run `cargo check`
3. Fix all compiler errors (they show exactly what to update)
4. Run tests to verify behavior

### Example Migration

**MVP**:
```rust
struct User {
    name: Option<String>,
    email: Option<String>,
    activated: bool,
}
```

**Production**:
```rust
enum UserState {
    Unregistered(UnregisteredUser),
    Registered(RegisteredUser),
    Activated(ActivatedUser),
}

struct UnregisteredUser { /* no name/email needed */ }
struct RegisteredUser { name: String, email: String }
struct ActivatedUser { name: String, email: String, activated_at: DateTime }
```

**Compiler helps**: Every `user.name` access becomes a compile error, showing exactly where to handle the state transition.

---

## Decision Framework

### When to Use Progressive Architecture

| Signal | Action |
|--------|--------|
| Business model unproven | Start with MVP patterns |
| Deadline < 2 weeks | MVP patterns acceptable |
| Core domain logic stable | Refactor to production patterns |
| Compiler errors are helpful | Embrace type-driven refactoring |

### When NOT to Use

| Scenario | Reason |
|----------|--------|
| Safety-critical systems (medical, aerospace, financial) | P0 requires type safety from start |
| Financial transactions | Invalid states must be impossible |
| Concurrent systems with shared state | Data races must be prevented at compile time |

---

## Checklist: Ready to Refactor?

Before refactoring from MVP to production:

- [ ] Business logic validated (product-market fit confirmed)
- [ ] Core domain entities identified (stable, won't change frequently)
- [ ] Test coverage > 80% for critical paths
- [ ] Team capacity available (refactoring takes time)
- [ ] No imminent deadlines (don't refactor before demo)

## Related

- [priority-pyramid.md](01-priority-pyramid.md) — When to prioritize what
- [conflict-resolution.md](02-conflict-resolution.md) — Specific conflict scenarios
- [trade-offs.md](04-trade-offs.md) — Decision analysis framework
