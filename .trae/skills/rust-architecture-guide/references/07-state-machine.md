---
title: "State Machine Design"
description: "Type-driven state machines for compile-time state transition guarantees"
category: "Patterns"
priority: "P1"
applies_to: ["standard", "strict"]
prerequisites: ["03-progressive-architecture.md"]
dependents: []
---

# State Machine Design

## When to Use State Machines

Use state machines for core business entities with clear lifecycle transitions.

### ✅ Use State Machines When:
- Core business entities have explicit lifecycle flows (orders, agreements, workflows)
- State transitions need compile-time guarantees
- Invalid state transitions should be impossible to represent

### ❌ Don't Use When:
- Simple independent boolean flags (`is_deleted`, `is_vip`)
- No complex flow or transitions

## Implementation Pattern

```rust
// ✅ Required: Enum with payload + ownership transfer
enum OrderState {
    Created(OrderInfo),
    Paid(PaymentInfo),
    Shipped(TrackingInfo),
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
}
```

## Trade-offs

**Compile-time safety vs Code complexity**: State machines prevent invalid states but add boilerplate. Use for core domains only.

**Ownership transfer**: Forces explicit state transitions, making flows clear but requiring more verbose patterns.

## Pragmatic Compromises

**Edge cases - Simple boolean flags**: For independent attributes that don't participate in complex flows (`is_deleted`, `is_vip`), use plain `bool` fields. Don't over-engineer.

```rust
// ✅ Pragmatic: Simple flags as bool
struct User {
    id: UserId,
    is_deleted: bool,  // Simple flag, no state machine needed
    is_vip: bool,
}
```

## Related

- [newtype.md](08-newtype.md) — For type-safe IDs within state machines
- [trade-offs.md](04-trade-offs.md) — Framework for deciding when state machines are worth the cost
