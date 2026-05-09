---
title: "API Design & Boundaries"
description: "Builder pattern, generics, forward compatibility with #[non_exhaustive], sealed traits"
category: "Patterns"
priority: "P1"
applies_to: ["standard", "strict"]
prerequisites: []
dependents: []
---

# API Design & Boundaries

## Builder Pattern

### Required Fields in `new()`, Optional in Builder

```rust
// ✅ Required fields in constructor
struct Config {
    required_field: String,
    optional_timeout: Option<Duration>,
}

impl Config {
    fn new(required_field: String) -> Self {
        Self {
            required_field,
            optional_timeout: None,
        }
    }

    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.optional_timeout = Some(timeout);
        self
    }
}
```

**Why**: Prevents runtime errors from missing required fields at `build()` time.

## Generic Parameters (Monomorphization Trade-offs)

### Decision Matrix

| API Type | Strategy | Rationale |
|---------|---------|-----------|
| Public API for external developers | Use `impl Into<String>` or `impl AsRef<Path>` | Ergonomics for callers |
| Internal modules, high-frequency helpers | Use concrete types (`&str`, `&Path`) | Limit monomorphization & compile time |

```rust
// ✅ Public API: Generic for ergonomics (better caller experience)
pub fn load_file(path: impl AsRef<Path>) -> Result<String> { }

// ✅ Internal: Concrete for compile speed (prefer concrete types for internal functions)
fn internal_helper(path: &Path) { }
```

**Trade-off**: Generics increase monomorphization and compile times. Limit over-generalization in internal code.

## Return Types: Concrete vs Dynamic Dispatch

### Priority: Concrete or `impl Trait`

```rust
// ✅ Preferred: Static dispatch for inlining
fn get_items() -> impl Iterator<Item = u32> { }
```

### Use `Box<dyn Trait>` When:

- Heterogeneous collections
- Deep recursion
- Complex conditional branches returning different iterators

```rust
// ✅ Don't fight static dispatch: Use Box<dyn Trait>
fn get_processor(config: &Config) -> Box<dyn Processor> {
    match config.algorithm {
        Algorithm::Fast => Box::new(FastProcessor),
        Algorithm::Accurate => Box::new(AccurateProcessor),
    }
}
```

**Trade-off**: Tiny heap allocation cost for clear API and faster compile times.

## Forward Compatibility: `#[non_exhaustive]`

### Pain Point

In large systems, downstream modules are often developed by different teams. If you add a variant to a public Enum or a field to a public Struct in v1.1, all downstream code breaks compilation due to "non-exhaustive patterns" or "missing field".

### Rule: Mark Evolving Public Types

**Specification**: For all externally exposed Enums and Structs that may evolve in the future, must mark as `#[non_exhaustive]`.

```rust
// ✅ Forward-compatible enum
#[non_exhaustive]
pub enum ServerEvent {
    Connected(ConnectionInfo),
    Disconnected(DisconnectReason),
    // Future variants can be added without breaking downstream
}

// Downstream MUST provide wildcard branch
match event {
    ServerEvent::Connected(info) => { /* ... */ },
    ServerEvent::Disconnected(reason) => { /* ... */ },
    _ => { /* handle future variants */ },
}
```

```rust
// ✅ Forward-compatible struct
#[non_exhaustive]
pub struct Config {
    pub port: u16,
    pub host: String,
    // Future fields can be added without breaking downstream
}

// Downstream MUST use Builder or struct update syntax, cannot construct directly
let config = Config {
    port: 8080,
    host: "localhost".to_string(),
    ..Config::default()
};
```

**Benefit**: Allows adding fields/variants without breaking downstream compilation. Downstream is forced to handle unknown cases.

## Sealed Trait Pattern

### Rule: Prevent External Implementations

**Specification**: If you define a Trait for internal dynamic dispatch but don't want users to implement it, must use the "sealed trait" pattern.

```rust
// ✅ Sealed trait: users cannot implement, only use
mod private {
    pub trait Sealed {}
}

pub trait Processor: private::Sealed {
    fn process(&self, data: &[u8]) -> Result<Output>;
}

// Only your crate can implement
impl private::Sealed for FastProcessor {}
impl Processor for FastProcessor {
    fn process(&self, data: &[u8]) -> Result<Output> { /* ... */ }
}

// ❌ Downstream CANNOT implement:
// impl Processor for CustomProcessor { } // Error: trait `Sealed` is private
```

**Architecture Benefits**:
- Prevents users from implementing custom logic that breaks your system invariants
- Preserves your right to add methods to the Trait without breaking compatibility
- Clear boundary: "this Trait is for internal dispatch, not for extension"

## Deprecation Migration: `#[deprecated]`

### Rule: Graceful Migration Path

When removing or renaming a public API, use `#[deprecated]` with a clear migration hint:

```rust
// ✅ Deprecated with migration path
#[deprecated(since = "2.1.0", note = "use `process_batch` instead — handles empty input correctly")]
pub fn process_all(items: &[Item]) -> Result<()> {
    process_batch(items)
}

// ✅ Deprecated type alias for renamed type
#[deprecated(since = "3.0.0", note = "renamed to `OrderId` — use the new name for clarity")]
pub type OrderId = crate::domain::OrderId;
```

### Migration Strategy

| Phase | Action | Duration |
|-------|--------|----------|
| 1. Deprecate | Add `#[deprecated]` with `note` pointing to replacement | 1 minor version |
| 2. Warn | Clippy `deprecated` lint fires for all users | 1+ minor versions |
| 3. Remove | Delete in next major version with changelog entry | Major bump |

**Rule**: Never remove a deprecated API in a minor/patch version — only in a major semver bump.

## Trait Object Safety

Not all traits can be used as `dyn Trait`. A trait is **object-safe** only if:

- All methods have `&self` or `&mut self` receiver
- No method returns `Self` (the concrete type is unknown at runtime)
- No method has generic type parameters (no `fn foo<T>(&self, t: T)`)
- The trait does not have `Sized` as a supertrait

```rust
// ❌ NOT object-safe: returns Self
trait Cloneable: Clone {
    fn duplicate(&self) -> Self;  // Error: returns Self
}

// ✅ Object-safe: all methods use &self, no Self in return
trait Processor {
    fn process(&self, data: &[u8]) -> Result<Output>;
    fn name(&self) -> &str;
}

// Can now use as dyn trait
let processors: Vec<Box<dyn Processor>> = vec![/* ... */];
```

**Rule**: If you need `dyn Trait`, design the trait for object safety from the start. Use associated types instead of generics where possible.

## Trade-offs

**Ergonomics vs Compile time**: Generic APIs are pleasant to use but slow compilation.

**Static vs Dynamic dispatch**: Static is faster but can lead to complex type signatures.

**Stability vs Flexibility**: `#[non_exhaustive]` adds minor boilerplate for downstream but guarantees forward compatibility.

**Object safety vs Generics**: Object-safe traits enable `dyn Trait` but restrict method signatures. Use associated types to bridge the gap.

## Rust API Guidelines Alignment

This document aligns with the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) checklist. Key mandatory items:

| Guideline | Rule | Our Mapping |
|-----------|------|-------------|
| **C-CASE** | Casing conforms to RFC 430 | Jeet Kune Do naming (ref/22) |
| **C-CONV** | `as_`, `to_`, `into_` conventions | Trait patterns (ref/20): `From`/`Into`/`AsRef` |
| **C-MACRO-VIS** | Item macros support visibility specifiers | Metaprogramming (ref/14) |
| **C-GOOD-ERR** | Good error types are meaningful | Error handling (ref/10): `#[non_exhaustive]`, structured enums |
| **C-NONEXHAUSTIVE** | Types are `#[non_exhaustive]` for evolution | This document §Forward Compatibility |
| **C-SEALED** | Traits are sealed where needed | This document §Sealed Trait Pattern |
| **C-STABLE** | SemVer compatible changes only | Deprecation protocol §Deprecation Migration |
| **C-EXAMPLE** | All items have rustdoc examples | `strict` mode: mandatory doc-tests |

> **Agent Directive**: When designing or reviewing a public API, cross-reference the [Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html) and note deviations in the Decision Summary.

## Related

- [error-handling.md](10-error-handling.md) — Error types in API boundaries
- [newtype.md](08-newtype.md) — Type-safe parameters in APIs
- [toolchain.md](17-toolchain.md) — Workspace and dependency management
- [traits.md](20-traits.md) — Trait design patterns and zero-cost abstractions
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — Official API design checklist
