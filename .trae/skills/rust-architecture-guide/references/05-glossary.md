# Glossary — Terminology Reference

This document centralizes definitions for specialized terms used across the Rust Architecture Guide. When the Agent encounters an unfamiliar term, it MUST consult this glossary rather than inferring meaning.

---

## Philosophy & Metaphor Terms

The guide uses metaphors from Eastern philosophy and martial arts to describe engineering principles. These are **conceptual tools**, not literal violence or ideology. Below are the mappings from metaphor to engineering reality.

### Quick Reference Table

| 中文术语 | English Term | Engineering Translation | Core Concept |
|---------|-------------|------------------------|--------------|
| 截拳道 | Jeet Kune Do | **Compile-time Defense First** | Intercept bugs through type systems before runtime |
| 唯物辩证法 | Dialectical Materialism | **Hardware Sympathy** | Acknowledge physical laws; Safe/Unsafe unity of opposites |
| 寸劲 | One-Inch Punch | **Batch Deallocation / Zero-Copy** | Maximum effect with minimal cost (Arena, bulk free) |
| 经济法则 | Economy of Motion | **Zero-Cost Abstraction** | Every line points directly to intent; no wasted motion |
| 截击之道 | Intercepting Fist | **Parse, Don't Validate** | Stop errors at compilation, not runtime |
| 对立统一 | Unity of Opposites | **Safety Through Abstraction** | Unsafe as foundation of safe APIs; error as resilience step |
| 防波堤 | Breakwater | **FFI Safety Boundary** | Block C chaos from reaching business layer |
| 对称分配 | Symmetric Allocation | **Allocator Ownership** | Who allocates must deallocate; no cross-boundary free |

> 🤖 **For AI Agents**: When encountering these terms, map them to their **Engineering Translation** equivalents. These describe type safety, borrow checking, zero-cost abstractions, and mechanical sympathy.

---

### Detailed Definitions

#### Jeet Kune Do (截拳道)
**Engineering Translation**: Compile-time Defense First

Bruce Lee's martial arts philosophy emphasizing efficiency, directness, and adaptation. Applied to Rust: eliminate boilerplate, write code that points directly to intent, adapt to hardware constraints.

**Core Tenets**:
- **Intercepting Boilerplate**: Stop repetitive patterns before they propagate (macros, generics, abstractions)
- **Economy of Motion**: Every line has purpose; no redundant clones or allocations
- **Form Without Form**: Adapt to the problem, don't force a pattern where it doesn't fit

**Rust Applications**:
- State machine enumization (invalid states unrepresentable)
- Newtype pattern (type safety without runtime cost)
- `let else` over nested `if let` (intercept complexity early)
- `#[non_exhaustive]` (preserve API evolution rights)

#### Dialectical Materialism (唯物辩证法)
**Engineering Translation**: Hardware Sympathy

Marxist philosophical framework applied to software: contradictions drive evolution, quantitative accumulation leads to qualitative leaps, and negation of negation leads to higher resilience.

**In Rust Engineering**:
- **Unity of Opposites**: `unsafe` is not the enemy of safe code but its material foundation. The `-sys` crate is unsafe so the wrapper can be safe.
- **Quantitative to Qualitative**: An MVP's `Option<bool>` flags accumulate until business model stabilizes, then undergo qualitative change to Enum state machine. The compiler assists by identifying all call sites.
- **Negation of Negation**: Errors are not endpoints but starting points — panic → catch → graceful degradation. Each negation reaches a higher level of resilience.

---

## Architecture & Design Terms

## Architecture & Design Terms

### Monomorphization Retreat
The deliberate switch from compile-time generic dispatch (`fn process<T: Trait>(...)`) to runtime dynamic dispatch (`fn process(Box<dyn Trait>)`) when generic monomorphization causes excessive compile time or binary bloat. This is a **pragmatic compromise** governed by P2 > P1 for non-safety-critical internal code. The term "retreat" emphasizes that generics are the default and dynamic dispatch is a fallback, not a first choice.

**Decision threshold**: Switch when compile time increases >2x or binary size grows >1.5x due to monomorphization.

### Sealed Trait Defense
A pattern where a public trait has a private super-trait (the "seal"), preventing external crates from implementing the trait. This defends the crate's invariants by ensuring only internally-approved types satisfy the trait.

```rust
mod private { pub trait Sealed {} }
pub trait Processor: private::Sealed { fn process(&self); }
// External crates cannot implement Processor — the Sealed supertrait is private
```

**Use when**: The trait is for internal dispatch only, not for user extension. Preserves the right to add methods without breaking downstream.

### Zero-Cost Abstraction Boundary
The line between abstractions that compile away entirely (zero runtime cost) and abstractions that introduce runtime overhead. In this guide, the boundary is explicitly managed: marker traits and `PhantomData` are on the zero-cost side; `Box<dyn Trait>` and runtime validation are on the cost-bearing side. The guide mandates that crossing this boundary requires justification via the priority pyramid.

### Algebraic Indexing
Using Rust's type system to encode indexing invariants at compile time, eliminating runtime bounds checks. Examples include: using `NonZeroUsize` to represent known-non-zero indices, newtype wrappers that guarantee valid index ranges, and const generics for fixed-size indexing. The term emphasizes that the index validity is proven algebraically (by type construction) rather than checked dynamically.

### Breakwater Principle
FFI architecture philosophy: the safe wrapper crate acts as a "breakwater" that absolutely blocks the chaos of C's raw pointers, manual memory management, and undefined behavior from reaching the business layer. All `unsafe` and raw pointers are confined to the `-sys` crate; the wrapper exposes only safe, idiomatic Rust APIs.

### Symmetric Allocation
FFI memory ownership rule: whoever allocates memory is responsible for deallocating it. Rust-allocated memory (`Box::into_raw`) must be reclaimed by Rust (`Box::from_raw`); C-allocated memory (`malloc`) must be freed by C (`free`). Cross-deallocation is **forbidden** as it causes undefined behavior across allocator boundaries.

### Opaque Handle
A pattern for FFI where a C or Rust struct is passed across the language boundary as a pointer to an incomplete type. The other side cannot inspect or modify the struct's fields — it can only hold the pointer and pass it back. Implemented in Rust as zero-sized types or private-struct pointers: `#[repr(C)] pub struct CxHandle { _private: [u8; 0] }`.

---

## Concurrency & Async Terms

### Work-Stealing Scheduler
Tokio's default multi-threaded runtime scheduler where idle threads "steal" tasks from busy threads' queues. This provides automatic load balancing but means a single Future's execution may hop across OS threads at each `.await` point. This is why traditional thread-based logging fails in async Rust — logs from one request scatter across multiple thread streams.

### Cancellation Safety
Whether a Future can be safely dropped (cancelled) at an `.await` point without losing data or corrupting state. A Future is cancellation-safe if partial progress is either fully committed or fully rolled back. Example: `tokio::sync::mpsc::Sender::send` is cancellation-safe (message either sent or not); `tokio::io::AsyncWrite::write` is NOT (partial writes possible).

### Select Bias
In `tokio::select!`, branches are checked in declaration order by default. The `biased;` directive disables random branch ordering, ensuring deterministic priority: the first branch is always checked first. Use `biased;` when one branch represents higher-priority work (e.g., shutdown signal should take precedence over new request acceptance).

### Spawn Blocking Bridge
The pattern of using `tokio::task::spawn_blocking` to offload blocking operations (file I/O, CPU-heavy computation, C library calls) from the async runtime's thread pool to a dedicated blocking thread pool. This prevents blocking operations from starving async tasks.

---

## Error Handling Terms

### Error Layering
The architectural separation of error handling into two distinct layers:
- **Library layer**: Uses `thiserror` for structured, typed errors with rich context. Public API never returns `anyhow::Error`.
- **Application layer**: Uses `anyhow` for convenient error propagation with `.context()` for business-level diagnostics.

This separation ensures library errors are matchable and self-documenting, while application code stays concise.

### Lazy Context vs Eager Context
- **Lazy context**: `.with_context(|| format!("Failed at {path}"))` — the closure is only evaluated if the Result is `Err`. Zero cost on the happy path.
- **Eager context**: `.context(format!("Failed at {path}"))` — the string is allocated unconditionally, even on `Ok`. Wastes CPU and memory on the happy path.

Always prefer lazy context in production code.

```rust
// ✅ Lazy: closure only runs on Err
result.with_context(|| format!("Failed to read {}", path.display()))?;

// ❌ Eager: string allocated even on Ok
result.context(format!("Failed to read {}", path.display()))?;
```

---

## Type System Terms

### Type-Driven State Machine
Encoding business state transitions in the type system so that invalid states are unrepresentable at compile time. Each state is a distinct enum variant carrying only the data valid for that state. Transitions return `Result<Self, TransitionError>`, and the compiler rejects invalid transitions.

```rust
enum OrderState {
    Created(OrderInfo),
    Paid(PaymentInfo),
    Shipped(TrackingInfo),
}
// Cannot call .ship() on Created — compiler enforces this
```

### Impossible States Unrepresentable
A design goal where the type system makes invalid state combinations impossible to construct. Contrast with runtime validation (`assert!(status == Expected)`), which catches bugs at test time but not at compile time. This is the production-grade target of the progressive architecture path.

### Newtype Pattern
Wrapping a type in a single-field struct to create a distinct type with its own invariants. Used for type-safe IDs (`struct UserId(Uuid)`), validated strings (`struct Email(String)`), and dimension types (`struct Meters(f64)`). Prevents mixing semantically different values that share the same underlying type.

### Interior Mutability
The ability to mutate data through a shared reference (`&T`), bypassing Rust's default exclusivity rule. Achieved via `Cell<T>` (for `Copy` types, single-threaded), `RefCell<T>` (for non-`Copy` types, single-threaded, runtime borrow checking), and `Mutex<T>`/`RwLock<T>` (multi-threaded). The "interior" means the mutation is contained within the cell's own safety guarantees.

---

## Performance Terms

### SoA Layout (Structure of Arrays)
Organizing data so that each field is stored in its own contiguous array, rather than storing complete structs contiguously (AoS — Array of Structures). SoA improves cache locality when accessing a single field across many elements, enabling SIMD vectorization.

```rust
// AoS: poor cache locality for single-field iteration
struct Particle { x: f32, y: f32, z: f32, mass: f32 }
let particles: Vec<Particle>;

// SoA: excellent cache locality for position-only iteration
struct ParticleSystem { x: Vec<f32>, y: Vec<f32>, z: Vec<f32>, mass: Vec<f32> }
```

### False Sharing
A performance degradation where multiple threads frequently write to different variables that happen to share the same CPU cache line (typically 64 bytes). The hardware cache coherency protocol forces unnecessary cache line invalidations across cores. Fix: align hot concurrent fields to separate cache lines with `#[repr(align(64))]`.

### Profile-Guided Optimization (PGO)
Using runtime profiling data (from representative workloads) to guide the compiler's optimization decisions: function layout, inlining thresholds, branch prediction. Can yield 5-15% performance improvement for latency-critical services. Requires: (1) build with instrumentation, (2) run representative workload, (3) rebuild with profile data.

---

## Observability Terms

### Span Isolation
Creating a `tracing::Span` for each logical request (HTTP request, message consumption) so that all events within that request are automatically correlated, even when the Future hops across threads in a work-stealing scheduler. Without Span isolation, logs from one request interleave with logs from other requests, making debugging impossible.

### W3C Trace Context
The W3C standard for propagating distributed tracing context across service boundaries via HTTP headers (`traceparent` and `tracestate`). Enables end-to-end request tracing across microservices written in different languages. Implemented in Rust via `tracing-opentelemetry`.

### Non-Blocking Appender
A log output mechanism (`tracing_appender::non_blocking`) that uses a lock-free concurrent queue to buffer log records, with a dedicated background thread performing the actual I/O. This ensures that disk I/O jitter or network latency in the logging pipeline never blocks business threads.

---

## FFI Terms

### Trampoline Pattern
The technique for passing Rust closures as C callbacks: a C-compatible "trampoline" function receives both the callback function pointer and a `user_data` pointer. The `user_data` points to the Rust closure's heap-allocated environment. The trampoline dereferences `user_data` to recover the closure and calls it. **Never** cast a Rust closure directly to a C function pointer — closures capture environment and are not C-compatible.

### Panic Containment
Wrapping all `extern "C"` function bodies in `std::panic::catch_unwind` to prevent Rust panics from crossing the FFI boundary. Unwinding across FFI is undefined behavior. If a panic is caught, return a C-compatible error code instead.

---

## Testing Terms

### Property-Based Testing
Testing by stating properties (invariants) that must hold for all inputs, then letting a framework (`proptest`) generate random inputs to find counterexamples. Contrast with example-based testing which only checks specific inputs. Essential for core algorithms where edge cases are numerous.

### Concurrency Model Checking
Using `loom` to systematically explore all possible thread interleavings of concurrent code, finding deadlocks and data races that probabilistic testing misses. `loom` replaces `std::sync::Atomic` with its own mock implementations that enumerate all possible execution orderings.

### Miri UB Detection
Running tests under the Miri interpreter to detect undefined behavior in unsafe Rust code: out-of-bounds access, use-after-free, invalid pointer casts, data races, and more. Miri interprets the program at the MIR level, checking every memory operation against Rust's operational semantics.

---

## Process Terms

### Progressive Architecture
The principle that architecture should adapt to the project lifecycle: MVP uses simple patterns (single struct + Option fields, runtime validation), and refactors to production patterns (type-driven state machines, compile-time guarantees) only after business logic is validated. The Rust compiler assists refactoring by identifying all affected call sites.

### Deviation
A formally documented exception to a rule in this guide. Marked with `// DEVIATION: reason` in code and recorded in the Decision Summary. In `strict` mode, deviations trigger additional review. Deviations are not violations — they are conscious, documented trade-offs.

### Decision Summary
A mandatory output block appended to every code generation or review response. Contains: applied rules, conflict resolutions, deviation records, and mode context. Ensures the Agent's reasoning is transparent and auditable.

---

## Related

- [00-mode-guide.md](00-mode-guide.md) — Execution modes that govern rule enforcement
- [priority-pyramid.md](01-priority-pyramid.md) — Priority levels referenced throughout
- [trade-offs.md](04-trade-offs.md) — Decision analysis framework
