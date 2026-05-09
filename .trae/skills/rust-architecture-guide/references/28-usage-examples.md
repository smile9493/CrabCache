# Usage Examples

## Scenario 1: Starting a New Rust Project

**Context**: Building a web API for e-commerce platform. Team of 5, 3-month timeline.

### Step 1: Invoke Master Guide

```
/rust-architecture-guide priority
```

**Outcome**: Understand P0-P3 priorities for your context.

### Step 2: Set Architecture Foundation

```
/rust-architecture-guide error-handling library
```

**Decision**: Since building both library (shared crates) and application (API binary):
- Library crates: `thiserror` for structured errors
- API binary: `anyhow` for error aggregation

### Step 3: Establish Coding Style

```
/rust-architecture-guide control-flow
```

**Adoption**: Team agrees on `let else` for early returns, `matches!` for enum checks.

---

## Scenario 2: Performance Optimization Request

**Context**: API latency spike at 99th percentile. Need to optimize.

### Step 1: Profile First

```bash
# Generate flamegraph
flamegraph ./target/release/api --benchmark
```

### Step 2: Analyze Hotspot

**Finding**: JSON serialization is 40% of request time.

### Step 3: Invoke Trade-off Analysis

```
/rust-architecture-guide conflict performance
```

**Applied**: P3 (performance) > P1 (elegance) for hot path

### Step 4: Implement Optimization

```rust
// Before: Generic serde serialization
fn serialize_response<T: Serialize>(data: &T) -> String {
    serde_json::to_string(data).unwrap()
}

// After: Zero-copy with bytes + manual serialization for hot path
fn serialize_response(data: &HotPathData) -> Bytes {
    // Manual serialization, avoids allocations
    // Justified: Called 10K times/sec, profiler shows 60% improvement
}
```

### Step 5: Document Decision

```markdown
## Decision: Manual Serialization for Hot Path

**Priority**: P3 > P1

**Evidence**: Flamegraph shows 40% time in serde_json
**Improvement**: 60% latency reduction at p99

**Scope**: Only for HotPathData, other types use serde
**Review**: Revisit if data structure changes
```

---

## Scenario 3: Code Review Conflict

**Context**: Team member submits PR with complex lifetime annotations.

### PR Code (Problematic)

```rust
pub struct UserContext<'a, 'b, 'c> {
    user: &'a User,
    config: &'b Config,
    cache: &'c Mutex<Cache>,
}
```

### Step 1: Apply Priority Pyramid

```
/rust-architecture-guide priority maintainability
```

**Analysis**: Violates P1 (maintainability) for P3 (performance) that doesn't exist.

### Step 2: Suggest Alternative

```rust
// ✅ Better: Owned types, clear ownership
pub struct UserContext {
    user: Arc<User>,
    config: Arc<Config>,
    cache: Arc<Mutex<Cache>>,
}
```

**Trade-off**: 
- ✅ P1: Much clearer, no lifetime pollution
- ⚠️ P3: Tiny Arc overhead (negligible, not in hot path)
- ✅ P2: Faster compilation (no lifetime inference)

### Step 3: Reference Style Guide

```
/rust-architecture-guide borrowing
```

Share with team: "Slices over vectors, owned over complex lifetimes in business logic."

---

## Scenario 4: MVP Deadline Pressure

**Context**: Need to ship MVP in 1 week. State machine pattern would take 3 days.

### Step 1: Invoke Progressive Architecture

```
/rust-architecture-guide conflict mvp
```

**Verdict**: MVP phase allows Enum + Options pattern.

### Step 2: Implement MVP Version

```rust
// ✅ MVP: Fast to implement
struct Order {
    status: OrderStatus, // Enum
    paid_at: Option<DateTime>,
    shipped_at: Option<DateTime>,
    // ... many Options
}

// Runtime validation
impl Order {
    fn mark_paid(&mut self) {
        assert_eq!(self.status, OrderStatus::Created);
        self.status = OrderStatus::Paid;
        self.paid_at = Some(Utc::now());
    }
}
```

### Step 3: Plan Refactoring

```markdown
## Technical Debt: Order State Machine

**Current**: Enum + Options (MVP pattern)
**Target**: Type-driven state machine
**Trigger**: After business validation (2-4 weeks post-launch)
**Effort**: 2-3 days (compiler helps refactoring)
```

---

## Scenario 5: FFI Integration

**Context**: Must integrate with high-performance C library for image processing.

### Step 1: Safety First

```
/rust-architecture-guide priority safety
```

**Rule**: P0 is absolute — must encapsulate unsafe.

### Step 2: Design Safe Wrapper

```
/rust-architecture-guide toolchain unsafe
```

**Implementation**:

```rust
// Internal FFI module
mod ffi {
    pub unsafe fn c_process_image(ptr: *mut u8, len: usize) -> i32;
}

// Public safe API
pub mod image {
    pub struct ImageProcessor { /* no raw pointers exposed */ }
    
    impl ImageProcessor {
        pub fn process(&self, image: &mut [u8]) -> Result<()> {
            // SAFETY: We guarantee pointer validity
            let result = unsafe {
                ffi::c_process_image(image.as_mut_ptr(), image.len())
            };
            
            if result == 0 {
                Ok(())
            } else {
                Err(Error::ProcessingFailed)
            }
        }
    }
}
```

### Step 3: Verify Encapsulation

**Check**: Can business code use `ImageProcessor` without `unsafe`?
- ✅ Yes: `processor.process(&mut image)?` is completely safe

---

## Scenario 6: Team Disagreement on Generics

**Context**: Team split on whether to use generics or `Box<dyn Trait>` for processor pipeline.

### Option A: Generics

```rust
fn process_all<I, P>(items: I)
where
    I: IntoIterator<Item = P>,
    P: Processor,
{ }
```

### Option B: Dynamic Dispatch

```rust
fn process_all(items: Vec<Box<dyn Processor>>) { }
```

### Resolution Process

```
/rust-architecture-guide trade-off generics
```

**Analysis**:

| Factor | Generics | Box<dyn Trait> |
|--------|---------|----------------|
| Compile Time | ❌ Slow (monomorphization) | ✅ Fast |
| Runtime | ✅ Fast (inlining) | ⚠️ Vtable lookup |
| Use Case | Public API | Internal pipeline |

**Decision**: Since internal pipeline, choose `Box<dyn Trait>` for:
- ✅ Faster compilation (P2)
- ✅ Heterogeneous processors (can't do with generics)
- ⚠️ Negligible runtime cost (not hot path)

---

## Quick Reference: When to Invoke What

| Scenario | Command |
|----------|---------|
| Starting new project | `/rust-architecture-guide priority` → `/rust-architecture-guide error-handling` |
| Performance issue | Profile → `/rust-architecture-guide conflict performance` |
| Code review | `/rust-architecture-guide review` |
| MVP deadline | `/rust-architecture-guide conflict mvp` |
| FFI integration | `/rust-architecture-guide priority safety` |
| Team disagreement | `/rust-architecture-guide trade-off [topic]` |
| Refactoring decision | `/rust-architecture-guide progressive-architecture` |

## Related

- [priority-pyramid.md](01-priority-pyramid.md) — The four-level hierarchy
- [conflict-resolution.md](02-conflict-resolution.md) — Specific scenarios
- [trade-offs.md](04-trade-offs.md) — Decision framework
