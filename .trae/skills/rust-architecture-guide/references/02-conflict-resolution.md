---
title: "Conflict Resolution Scenarios"
description: "Specific conflict scenarios and resolution strategies for P0-P3 priority violations"
category: "Architecture"
priority: "P0-P3"
applies_to: ["rapid", "standard", "strict"]
prerequisites: ["01-priority-pyramid.md"]
dependents: []
---

# Conflict Resolution Scenarios

## Conflict 1: Performance vs Elegance

### Scenario
String processing at scale. Zero-copy with complex lifetimes vs simple String with cloning.

### Resolution: 80/20 Rule

**Decision Matrix**:

| Path Type | Priority | Strategy | Code Pattern |
|-----------|---------|----------|--------------|
| Cold path (config, validation) | P1 > P3 | Clone boldly | `String` |
| Hot path (protocol parsing) | P3 > P1 | Zero-copy | `&'a str` or `bytes::Bytes` |

### Cold Path Example

```rust
// ✅ P1 > P3: Clone is fine for config loading
fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)?; // Allocates
    Config::parse(content) // Clone internally, doesn't matter
}
```

### Hot Path Example

```rust
// ✅ P3 > P1: Zero-copy for network parsing
use bytes::{Bytes, BytesMut};

fn parse_frames(buffer: &mut BytesMut) -> Vec<Frame> {
    // Zero-copy: Frame borrows from buffer
    // Critical for high-throughput networking
}
```

### Profiling Requirement

Before choosing P3 over P1:

```bash
# Must have profiler evidence
flamegraph ./target/release/myapp
```

**Rule**: No profiling = no optimization.

---

## Conflict 2: Macro Magic vs Compile Time

### Scenario
Powerful procedural macro reduces code duplication but build time: 1min → 5min.

### Resolution: P2 > P1

**Verdict**: Code reduction at 5x compile time cost is a **failed abstraction**.

### Anti-Pattern

```rust
// ❌ Heavy procedural macro
#[automagic_routes(
    get = "/users",
    post = "/users",
    put = "/users/{id}",
    delete = "/users/{id}",
    // ... 50 more routes
)]
pub struct UserRoutes;

// Compile time impact: +4 minutes
```

### Solutions

#### 1. Replace with macro_rules!

```rust
// ✅ Lightweight declarative macro
macro_rules! define_routes {
    ($($method:ident $path:literal => $handler:ident),* $(,)?) => {
        $(
            pub fn $method() -> Route {
                Route::new($path, $handler)
            }
        )*
    };
}
```

#### 2. Confine to Base Crate

```toml
# Cargo.toml
# Heavy macro only in rarely-changed base crate
[dependencies]
route-macros = { path = "crates/route-macros", version = "0.1" }
```

#### 3. Retreat to Conventional Code

```rust
// ✅ Boring but fast
pub fn register_user_routes(router: &mut Router) {
    router.get("/users", get_users);
    router.post("/users", create_user);
    // Explicit, clear, compiles fast
}
```

---

## Conflict 3: FFI/Interop vs Type Safety

### Scenario
Must call high-performance C library with raw pointers and global mutable state.

### Resolution: Establish "Safety Isolation Zone" with P0 Protection

**Absolute Rules**:
1. **Never** expose C structs with raw pointers to business layer
2. Build **thin Safe Wrapper** between C and Rust
3. Internal `unsafe`, but public API must be 100% safe

### Anti-Pattern

```rust
// ❌ FORBIDDEN: Exposing C API directly
pub struct CLibrary {
    pub handle: *mut libc::c_void, // Raw pointer exposed!
}

// Business code must deal with unsafe pointer
```

### Correct Pattern

```rust
// ✅ Safe wrapper architecture

mod ffi {
    // Internal unsafe FFI bindings
    #[link(name = "clib")]
    extern "C" {
        fn c_create() -> *mut libc::c_void;
        fn c_process(handle: *mut libc::c_void, data: *mut u8);
        fn c_destroy(handle: *mut libc::c_void);
    }
}

pub mod safe_api {
    use super::ffi;
    use std::ptr::NonNull;

    // Safe wrapper
    pub struct CLibrary {
        handle: NonNull<libc::c_void>, // Owned, not raw
    }

    impl CLibrary {
        pub fn new() -> Result<Self> {
            let handle = unsafe { ffi::c_create() };
            Ok(Self {
                handle: NonNull::new(handle)
                    .ok_or(Error::CreationFailed)?,
            })
        }

        pub fn process(&mut self, data: &mut [u8]) -> Result<()> {
            // SAFETY: We guarantee handle is valid and data outlives call
            unsafe {
                ffi::c_process(self.handle.as_ptr(), data.as_mut_ptr());
            }
            Ok(())
        }
    }

    impl Drop for CLibrary {
        fn drop(&mut self) {
            unsafe { ffi::c_destroy(self.handle.as_ptr()) }
        }
    }
}
```

---

## Conflict 4: Strong Typing vs Development Speed

### Scenario
State machine pattern requires 10 structs for 10 order states. MVP ships tomorrow.

### Resolution: Progressive Architecture

**Verdict**: Standards should not lock progress. MVP allows compromises.

### MVP Phase (Speed Priority)

```rust
// ✅ MVP: Pragmatic single struct
struct Order {
    id: OrderId,
    status: OrderStatus, // Enum: Created, Paid, Shipped, ...
    
    // All possible fields, most are Option
    paid_at: Option<DateTime<Utc>>,
    payment_id: Option<PaymentId>,
    shipped_at: Option<DateTime<Utc>>,
    tracking_number: Option<String>,
    delivered_at: Option<DateTime<Utc>>,
    
    // Validation via asserts in tests
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

### Post-MVP Refactoring (Type Safety Priority)

```rust
// ✅ Production: Type-driven state machine
enum OrderState {
    Created(OrderInfo),
    Paid(PaymentInfo),
    Shipped(ShipmentInfo),
    Completed,
}

// Compiler enforces valid transitions
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

### Why Progressive Architecture Works

1. **Compiler-assisted refactoring**: Rust tells you exactly what to update
2. **Business validation first**: Confirm product-market fit before over-engineering
3. **Test coverage**: Asserts in MVP become type invariants in production

### Migration Path

```
MVP (Week 1)
├─ Single struct with enum + Options
├─ Runtime validation via asserts
└─ Comprehensive test suite

↓ (After business validation)

Production (Week 3-4)
├─ Type-driven state machine
├─ Compile-time transition guarantees
└─ Impossible states unrepresentable
```

---

## Decision Template

When facing any conflict, use this template:

```markdown
## Conflict Analysis

**Scenario**: [Describe situation]

**In Conflict**:
- [ ] P0 (Safety)
- [ ] P1 (Maintainability)
- [ ] P2 (Compile Time)
- [ ] P3 (Runtime Performance)

**Priority Applied**: PX > PY

**Justification**: [Why this priority ordering]

**Profiler Evidence**: [If P3 involved, include data]

**Alternative Considered**: [What else was evaluated]

**Decision**: [Chosen approach]
```

## Related

- [priority-pyramid.md](01-priority-pyramid.md) — The four-level hierarchy
- [progressive-architecture.md](03-progressive-architecture.md) — MVP to production migration
- [trade-offs.md](04-trade-offs.md) — Decision framework
