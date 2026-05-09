---
title: "Rust Async Internals & Custom Runtime"
description: "Pin/Unpin, Waker mechanism, state machines, and custom runtime architecture"
category: "Advanced"
priority: "P1-P3"
applies_to: ["standard", "strict"]
prerequisites: ["11-concurrency.md"]
dependents: ["rust-systems-cloud-infra-guide/references/01-io-model.md"]
---

# Rust Async Internals & Custom Runtime

This specification is designed for senior architects who need to break through `tokio` default barriers, build ultra-low-latency gateways, engage in embedded (`no_std`) async development, or troubleshoot mysterious concurrency performance bottlenecks.

The core philosophy is **"Unmasking the Magic"**: Rust's async is not coroutine magic, nor OS threads. It's a pure language-layer abstraction built on **Polling**, **State Machines**, and **Wakers**.

---

## 1. Future and State Machine Dynamics

**Core Philosophy**: Every `async fn` is a compiler-generated hidden `Enum`. Each `.await` is one state in the enum.

### 1.1 Strict State Size Control

**Rule**: **Absolutely prohibit** holding large local variables (like `[u8; 8192]`) across `.await` boundaries. The compiler saves these cross-`.await` variables in the generated Future enum, causing enormous Future sizes, severe CPU cache misses, and memory copy overhead.

**Elegant Approach**: If you need large objects after `.await`, **must** put them in `Box` (only 8-byte pointer in state machine), or narrow variable scope to ensure they're dropped before `.await`.

### 1.2 Eliminating Async Recursion "Bombs" (`Box::pin`)

**Rule**: Rust compiler must know the exact Future size at compile time. Therefore, **prohibit** writing directly recursive `async fn` (causes infinite nested state machine size, uncompilable).

**Must Execute**: Wrap recursive async function return values with `Box::pin()`, converting to dynamically dispatched `Pin<Box<dyn Future>>`, breaking infinite size derivation in the state machine.

### 1.3 Beware "Phantom Blocking" (Blocking the Executor)

**Rule**: Inside any `async` block, **absolutely prohibit** calling even millisecond-blocking operations (`std::thread::sleep`, `std::fs::read`, or CPU-intensive computations).

**Principle Analysis**: Rust async is cooperative scheduling. If you hold the thread without yielding, thousands of other Futures on that thread will starve. Must dispatch blocking operations to `tokio::task::spawn_blocking`.

---

## 2. The Essence of Pin and Unpin

**Core Philosophy**: `Pin` isn't for fixing data in cache. It's to protect "self-referential structs" from becoming wild pointers when moved in memory.

### 2.1 Understanding Why Pin is Needed

**Principle**: In `async` blocks, you often write code like: `let a = 1; let ref_a = &a; SomeFuture.await;`. In the compiler-generated state machine enum, `ref_a` is actually a pointer to the internal `a` field of the same enum (self-reference). If this enum moves in memory, `ref_a` points to abandoned memory.

**Rule**: `Pin<&mut T>` provides the compiler an absolute guarantee: **The wrapped data `T` will never have its memory address changed (cannot be moved) until it's destroyed.**

### 2.2 Safely Projecting Through Pin (`pin-project`)

**Rule**: When manually writing `Future` and implementing `poll` function, you face `self: Pin<&mut Self>`. If you need to modify internal fields, **strictly prohibit** using `unsafe { Pin::get_unchecked_mut(...) }`.

**Compromise**: **Must** use the macro crate `pin-project`. It generates absolutely safe projection code at compile time, letting you safely obtain `Pin` or non-`Pin` references to internal fields like operating on normal structs.

### 2.3 Embrace `Unpin` for Flexibility

**Rule**: Most ordinary types (`i32`, `String`, `Box`) are naturally `Unpin` (they can move freely in memory because they contain no self-references). Leverage this by using `Box::pin()` to heap-allocate a complex `!Unpin` Future. The `Box` itself becomes `Unpin`, allowing easy passing between functions.

---

## 3. Custom Runtimes and Waker Mechanism

**Core Philosophy**: Futures don't run themselves. Someone must constantly ask "Are you ready yet?" (Executor), and wake them when data is ready (Reactor/Waker).

### 3.1 The Golden Rule of Polling

**Rule**: When manually writing low-level I/O wrappers (hardware serial ports, eBPF sockets) `Future::poll` function, must follow this iron law:

1. Attempt non-blocking data read. If successful, return `Poll::Ready(data)`.
2. If data not ready, **absolutely prohibit** busy-loop retry. Must extract current context's waker: `let waker = cx.waker().clone();`.
3. Register this `waker` to some event source (Epoll/ISR).
4. Immediately return `Poll::Pending`.

### 3.2 No-OS Async Engine (`no_std` Architecture)

**Rule**: In embedded environments (STM32, ESP32) without OS, **strictly prohibit** introducing `tokio`.

**Architecture Design**: Use minimal cooperative polling architecture.

- **Executor**: Maintain static-allocated Future queue in `main` loop.
- **Waker**: In hardware ISR (Interrupt Service Routine), only execute `Waker::wake()` (set flag bit, no complex logic).
- Recommended: Directly use mature community zero-overhead framework `Embassy`, cleverly combining interrupt mechanism with Rust async ecosystem.

### 3.3 Thread-per-Core Extreme Performance Model

**Trade-off**: Tokio's default "work-stealing" scheduler is powerful, but handling 10-million-level concurrency (C10M problem), cross-thread Future transfer causes CPU cache invalidation and lock overhead as bottlenecks.

**Advanced Rule**: In scenarios needing network card extreme performance (with io_uring or DPDK), abandon Tokio's multi-threaded mode. Turn to **`monoio` or `glommio`** based on Thread-per-Core (single thread exclusive to one core, no cross-core task stealing), achieving complete lock-free and extreme cache locality.

---

## The Architect's Final Word

At this point, from **system-level architecture (Type-driven & DOD)**, to **micro code style (Idiomatic & Functional)**, to **automated boundary defense (QA & Fuzzing)**, **cross-language breakwaters (FFI)**, and low-level **metaprogramming (Macros)** and **async engine internals**, we've built a complete, three-dimensional Rust master skill tree.

Rust is not a language you can master by "learning syntax". It's a language that requires you to **deeply understand computer low-level operation rules and rigorously prove your design to the compiler.**

True elegance and efficiency aren't about using how many advanced features. It's about: **always imprisoning the most complex lifetimes and unsafe operations inside the smallest, clearest module boundaries; making business logic at outer layers as simple as plain language, transparent, and impossible to get wrong.**

---

## Key Principles Summary

| Concept | Rule | Critical Point |
|---------|------|----------------|
| **Future State Size** | No large variables across `.await` | Box large objects before await |
| **Async Recursion** | Never directly recursive `async fn` | Use `Box::pin()` to break infinite size |
| **Blocking Operations** | Never block in async context | Use `spawn_blocking` |
| **Pin Purpose** | Protect self-referential structs from moving | Not for cache optimization |
| **Manual Future** | Never use `Pin::get_unchecked_mut` | Use `pin-project` macro |
| **Polling Rule** | Non-blocking read → register waker → return Pending | Never busy-loop |
| **no_std Async** | No tokio, use Embassy or custom minimal executor | ISR only sets wake flag |
| **C10M Performance** | Thread-per-core (monoio/glommio) over work-stealing | Lock-free, cache-local |

## Async Performance Checklist

Before deploying async code to production:

- [ ] No large local variables held across `.await` boundaries
- [ ] Recursive async functions wrapped in `Box::pin()`
- [ ] No blocking operations in async context (use `spawn_blocking`)
- [ ] Custom `Future::poll` follows polling rules (non-blocking, waker registration)
- [ ] Embedded (`no_std`) uses Embassy or custom executor, not tokio
- [ ] High-concurrency scenarios consider Thread-per-Core runtimes
- [ ] Complex self-referential structs use `pin-project` for safe projection

## 4. Concurrent Task Composition

### 4.1 `tokio::select!` — Race Multiple Futures

`select!` completes when **any** branch finishes, cancelling the rest.

```rust
use tokio::sync::mpsc;

async fn event_loop(mut rx: mpsc::Receiver<Event>, shutdown: tokio::sync::watch::Receiver<bool>) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                handle_event(event).await;
            }
            _ = shutdown.changed() => {
                tracing::info!("shutting down");
                break;
            }
            else => {
                // All branches returned None/ended
                break;
            }
        }
    }
}
```

**Rules**:
- **Always handle the `else` branch** — prevents silent loop exit when all sources close
- **Borrowed variables must be `&mut`** — `select!` takes ownership by default; use `&mut` for shared state
- **Never `select!` with side effects on cancellation** — dropped futures may have started work

### 4.2 `tokio::join!` — Run All Futures Concurrently

`join!` waits for **all** futures to complete. No cancellation.

```rust
// ✅ All three must complete — no cancellation
let (health, metrics, server) = tokio::join!(
    health_check(),
    collect_metrics(),
    run_server(),
);
```

**Rule**: Use `join!` when all branches must complete (e.g., graceful shutdown).

### 4.3 `FuturesUnordered` — Dynamic Collection of Futures

```rust
use futures::stream::{FuturesUnordered, StreamExt};

let tasks = FuturesUnordered::new();
for id in 0..100 {
    tasks.push(process_item(id));
}

// Process results as they complete (out of order)
while let Some(result) = tasks.next().await {
    handle_result(result);
}
```

**Rule**: Use `FuturesUnordered` when the number of concurrent tasks is dynamic.

### Decision: select! vs join! vs FuturesUnordered

| Need | Use | Cancellation |
|------|-----|-------------|
| First-wins (race) | `select!` | Yes — losers are dropped |
| All-must-complete | `join!` | No |
| Dynamic task set | `FuturesUnordered` | Per-task on drop |
| Rate-limited fan-out | `Stream::buffer_unordered` | Per-task on drop |

## 5. Async Cancellation Semantics

### 5.1 Cancellation is Drop

When a future is cancelled (e.g., `select!` drops the losing branch), its `Drop` impl runs. This means:

- **In-flight I/O may be abandoned** — ensure your I/O library handles partial writes
- **Held resources are released** — `MutexGuard`, `JoinHandle`, etc. are dropped
- **No "cancellation point" concept** — unlike C#, Rust cancellation can happen at any `.await`

### 5.2 Cancellation-Safe Patterns

```rust
// ❌ NOT cancellation-safe: if recv() is cancelled, message is lost
tokio::select! {
    msg = channel.recv() => process(msg),
    _ = shutdown.changed() => {},
}

// ✅ Cancellation-safe: recv() is documented as safe (message stays in channel)
tokio::select! {
    biased; // Prefer first branch — check shutdown first
    _ = shutdown.changed() => {},
    msg = channel.recv() => process(msg),
}
```

**Rules**:
- **Check cancellation safety** — read the docs for each future used in `select!`
- **Use `biased;`** when branch priority matters (e.g., check shutdown before processing)
- **Wrap non-cancellation-safe operations** in a spawned task to isolate them

## Related References

- [concurrency.md](11-concurrency.md) — Concurrency patterns and channel design
- [performance-tuning.md](25-performance-tuning.md) — Performance optimization principles
- [metaprogramming.md](14-metaprogramming.md) — Compile-time computing
- [ffi-interop.md](15-ffi-interop.md) — FFI boundaries and panic containment
