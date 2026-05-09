# Priority Pyramid Framework

## The Four-Level Hierarchy

When any conflict arises, apply decisions top-down. Upper levels have absolute veto power.

```
        P0: Safety & Correctness
        (Memory Safety, Data Consistency)
                    ↓
        P1: Maintainability
        (Readability, Complexity Control)
                    ↓
        P2: Compile Time
        (Build Speed, CI/CD Velocity)
                    ↓
        P3: Runtime Performance
        (Only in Proven Bottlenecks)
```

## P0: Absolute Red Line — Safety & Correctness

**Non-negotiable**

### Forbidden
- Using `unsafe` to bypass lifetime checks for convenience
- Accepting data races for performance
- Allowing dangling pointers

### Required
- Prefer locks or cloning over potential data races
- Always encapsulate unsafe in safe wrappers
- Memory safety guaranteed by compiler or proof

### Example Violations

```rust
// ❌ FORBIDDEN: unsafe for convenience
fn get_first(vec: &[i32]) -> i32 {
    unsafe { *vec.get_unchecked(0) } // Don't bypass borrow checker
}

// ✅ REQUIRED: Safe approach
fn get_first(vec: &[i32]) -> Option<i32> {
    vec.first().copied()
}
```

## P1: Core Requirement — Maintainability

**Highest principle for business code**

### Guidelines
- Code is for team and future self
- When "zero-cost abstractions" create cognitive overload, compromise
- Prefer owned types over complex lifetimes in business logic

### Example Trade-offs

```rust
// ❌ AVOID: Lifetime pollution in business structs
struct User<'a, 'b, 'c> {
    name: &'a str,
    config: &'b Config,
    cache: &'c Cache,
}

// ✅ PREFER: Owned types for clarity
struct User {
    name: String,
    config: Arc<Config>,
    cache: Arc<Mutex<Cache>>,
}
```

## P2: Engineering Efficiency — Compile Time

**Team survival metric**

### Warning Signs
- Build time > 5 minutes
- CI/CD blocked by type system complexity
- Generic monomorphization explosion

### Compromise Strategies
1. Replace complex generics with `Box<dyn Trait>`
2. Reduce procedural macro usage
3. Confine heavy type-level programming to base crates

### Measurement

```bash
# Track compile times
cargo +nightly -Z timings

# Identify slow crates
cargo build --timings
```

## P3: Runtime Performance

**Only at proven bottlenecks**

### Requirements Before Optimization
1. ✅ Profiler data (Flamegraph, perf)
2. ✅ Identified as CPU hotspot or memory bottleneck
3. ✅ Impact quantified (>10% improvement)

### Allowed Optimizations (After Profiling)
- Use `unsafe` for bounds check elimination
- Cache-friendly data structures
- Avoid micro-allocations in tight loops
- Inline assembly for critical paths

---

## Application Examples

### Example 1: String Handling Decision

**Scenario**: Processing string data

**Decision Tree**:
```
Is this a hot path? 
├─ No (config, init) → P1 > P3 → Use String, clone freely
└─ Yes (parsing, transformation)
   ├─ Profiler shows bottleneck? 
   │  ├─ No → P1 > P3 → Use String for simplicity
   │  └─ Yes → P3 > P1 → Use &str or bytes::Bytes
```

### Example 2: Concurrency Pattern

**Scenario**: Sharing state across threads

**Decision**:
```rust
// P0 Check: Must be thread-safe
// P1 Check: Must be understandable

// ✅ Correct approach
use std::sync::Arc;

let shared = Arc::new(Mutex::new(State::default()));

// ❌ Wrong: Unsafe for "performance"
let shared = unsafe { ... }; // Never for convenience
```

## Related

- [conflict-resolution.md](02-conflict-resolution.md) — Specific conflict scenarios
- [trade-offs.md](04-trade-offs.md) — Detailed trade-off analysis
