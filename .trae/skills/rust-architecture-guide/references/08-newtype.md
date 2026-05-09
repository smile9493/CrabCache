---
title: "Newtype Pattern"
description: "Type-safe wrappers for semantic isolation of domain IDs and credentials"
category: "Patterns"
priority: "P1"
applies_to: ["standard", "strict"]
prerequisites: []
dependents: []
---

# Newtype Pattern

## When to Use Newtype

Use newtype pattern for domain IDs, credentials, and basic types requiring semantic isolation.

### ✅ Use Newtype When:
- Domain identifiers (UserId, OrderId, ProductId)
- Security-sensitive values (ApiKey, Token, Password)
- Preventing parameter confusion in function arguments

### ❌ Don't Use When:
- Simple primitive values without semantic meaning
- Internal implementation details

## Implementation Pattern

```rust
// ✅ Recommended: Tuple struct for semantic isolation
struct UserId(u64);
struct OrderId(u64);

// ✅ With derive_more or nutype for automatic traits
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ApiKey(String);

// ⚠️ Critical: Don't implement Deref casually
// This breaks semantic isolation
```

## Best Practices

1. **Use macro crates** for boilerplate (strongly recommended):
   - `derive_more` — Automatic trait derivations (`Display`, `Serialize`, etc.)
   - `nutype` — Validated newtypes with constraints
   
   ```rust
   // ✅ Recommended: Use macros for automatic traits
   #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
   struct UserId(u64);
   ```

2. **Avoid Deref** unless absolutely necessary (critical constraint):
   ```rust
   // ❌ Breaks type safety - don't do this
   impl Deref for UserId {
       type Target = u64;
       fn deref(&self) -> &Self::Target { &self.0 }
   }
   ```
   
   **Why**: Implementing `Deref` breaks semantic isolation, allowing accidental arithmetic or comparisons across different newtypes.

3. **Implement only needed traits**:
   ```rust
   impl Display for UserId {
       fn fmt(&self, f: &mut Formatter) -> fmt::Result {
           write!(f, "user_{}", self.0)
       }
   }
   ```

## Trade-offs

**Type safety vs Convenience**: Newtypes prevent bugs but require explicit conversion.

**Compile time**: Each newtype adds minimal overhead, but many newtypes can increase compile times.

## Related

- [state-machine.md](07-state-machine.md) — Using newtypes within state machines
- [api-design.md](13-api-design.md) — Newtypes in public API boundaries
