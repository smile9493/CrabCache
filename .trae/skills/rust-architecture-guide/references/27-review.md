---
title: "Comprehensive Review Checklist"
description: "Code review checklist covering strategic, architectural, and style concerns"
category: "Process"
priority: "P0-P3"
applies_to: ["rapid", "standard", "strict"]
prerequisites: ["01-priority-pyramid.md", "00-mode-guide.md"]
dependents: []
---

# Comprehensive Review Checklist

## Code Review Process

Use this checklist when reviewing Rust code for both architecture and style.

## Strategic (Priority Pyramid)

- [ ] P0 not violated (no unsafe for convenience)
- [ ] P1 respected (code is maintainable, no over-engineering)
- [ ] P2 measured (compile time impact acceptable)
- [ ] P3 proven (performance optimizations have profiler data)

## Architecture Patterns

### Type System & State

- [ ] Core business entities use state machines where appropriate
- [ ] Simple boolean flags for independent attributes
- [ ] Newtype pattern for domain IDs and credentials
- [ ] Deref not implemented on newtypes (breaks isolation)
- [ ] Generics controlled to prevent monomorphization explosion

### Data Architecture

- [ ] Global entities use flat storage + indexing
- [ ] Smart pointers confined to local subgraphs
- [ ] Complex lifetimes only in low-level libraries
- [ ] Business logic uses owned types, avoids lifetime annotations
- [ ] Cloning used pragmatically in cold paths

### Error Handling

- [ ] Library code: structured error enums (thiserror)
- [ ] Application code: anyhow for aggregation
- [ ] No `anyhow::Error` in public library APIs
- [ ] Zero panic in business logic
- [ ] `expect()` with SAFETY comments where proven safe
- [ ] Lazy context attachment for expensive formatting

### Concurrency

- [ ] Message passing preferred over shared state
- [ ] Bounded channels for backpressure
- [ ] Minimal critical sections with locks
- [ ] No `std::sync::Mutex` held across `.await`
- [ ] `tokio::sync::Mutex` only when necessary

### API Design

- [ ] Required fields in `new()`, optional in builder
- [ ] Concrete types for internal functions
- [ ] `impl Into` for public API ergonomics
- [ ] `impl Trait` or concrete returns preferred
- [ ] `Box<dyn Trait>` used when static dispatch is complex
- [ ] `#[non_exhaustive]` on evolving public Enums and Structs
- [ ] Sealed trait pattern for internal-only Traits

### Zero-Cost Abstraction Boundaries

- [ ] Marker traits used for compile-time constraints (not runtime checks)
- [ ] `PhantomData` used only for well-understood patterns (no cryptic chains)
- [ ] Monomorphization retreat to `Box<dyn Trait>` when compile cost too high

## Coding Style

### Control Flow

- [ ] `let else` used to flatten nested `if let`/`match`
- [ ] `matches!` macro used for enum variant checks
- [ ] Pattern matching destructures directly in `let` bindings
- [ ] Guard clauses used appropriately with patterns

### Iterators

- [ ] No manual `for` + `push` patterns
- [ ] Iterator chains used for transform/filter/collect
- [ ] `filter_map` used when mapping might fail
- [ ] Zero unnecessary `.clone()` in chains

### Traits

- [ ] `From` implemented, not `Into`
- [ ] `Default` derived instead of parameterless `new()`
- [ ] Standard traits derived first (`Debug`, `Clone`, `PartialEq`)
- [ ] No unnecessary trait implementations

### Error Handling

- [ ] `unwrap_or_else` for expensive defaults
- [ ] `map_err` and `and_then` for chaining
- [ ] No verbose `match` for Result decomposition
- [ ] Appropriate use of `?` operator

### Data Structures

- [ ] Field initialization shorthand used
- [ ] No type stuttering (e.g., `user::UserConfig`)
- [ ] Struct update syntax with `Default`
- [ ] Newtype pattern for semantic types

### Borrowing & Mutability

- [ ] `mut` scope minimized via shadowing
- [ ] Slices (`&[T]`, `&str`) preferred over owned references
- [ ] No unnecessary cloning in loops
- [ ] Appropriate use of borrowing in iterators

## Toolchain Compliance

- [ ] Clippy passes with `-D warnings`
- [ ] Exceptions documented with comments
- [ ] Unsafe blocks have SAFETY comments
- [ ] Unsafe encapsulated in safe wrappers
- [ ] Doc-tests for public APIs only
- [ ] Cargo Workspace used for multi-crate projects
- [ ] Feature flags isolate heavy dependencies (serde, tokio)
- [ ] `cargo deny check` passes (no vulnerabilities, license compliance)

## Quick Scoring

**Excellent** (90-100% checks): Code is production-ready

**Good** (70-89% checks): Minor improvements needed

**Needs Work** (<70% checks): Significant refactoring recommended

## Common Anti-Patterns

### ❌ Indentation Hell

```rust
if let Some(x) = get_x() {
    if let Some(y) = get_y() {
        match z {
            Ok(val) => {
                // Core logic buried deep
            }
        }
    }
}
```

**Fix**: Use `let else` for early returns.

### ❌ Mutable Everywhere

```rust
let mut result = Vec::new();
for item in items {
    if condition {
        result.push(transform(item));
    }
}
```

**Fix**: Use iterator chains.

### ❌ Type Stuttering

```rust
use config::ConfigOptions;
let options = ConfigOptions::new();
```

**Fix**: Use module scope: `use config::Options;`

### ❌ Lifetime Pollution

```rust
struct UserContext<'a, 'b, 'c> {
    user: &'a User,
    config: &'b Config,
    cache: &'c Cache,
}
```

**Fix**: Use owned types: `Arc<User>`, `Arc<Config>`.

## Related

- [priority-pyramid.md](01-priority-pyramid.md) — Strategic framework
- [control-flow.md](18-control-flow.md) — Control flow patterns
- [iterators.md](19-iterators.md) — Iterator best practices
