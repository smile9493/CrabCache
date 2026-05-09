# Execution Mode Guide

## Overview

All rules in this Skill are not absolute — they must be applied relative to the project's lifecycle stage. The three execution modes determine **which priority levels are enforced** and **which rules are relaxed**, preventing the Agent from rigidly applying production-grade rules to prototype code.

**Default mode**: `standard`

---

## Mode Definitions

### `rapid` — Prototype / Hackathon / Proof of Concept

**Goal**: Validate business hypothesis at maximum speed.

**Enforced Priorities**: P0 (Safety & Correctness) only

**Relaxed Rules**:

| Rule | Rapid Mode Behavior |
|------|-------------------|
| Error handling in libraries | `anyhow` allowed everywhere, including library crates |
| `.clone()` usage | Unlimited cloning; no hotpath/coldpath distinction required |
| State machine design | `bool` flags + `Option` fields acceptable; no type-driven state machine required |
| Doc comments & doc-tests | Not required; `// TODO: add docs` is sufficient |
| `#[non_exhaustive]` | Not required on public types |
| Sealed traits | Not required |
| Tracing instrumentation | Optional; `println!` debugging acceptable |
| CI/CD checks | `cargo check` + `cargo test` only; no clippy/miri/deny |
| Unsafe code | Still requires SAFETY comments (P0 is never relaxed) |

**Mandatory Even in Rapid**:
- No undefined behavior (P0)
- No data races (P0)
- All `unsafe` blocks have SAFETY comments (P0)
- `unwrap()` only with `expect("reason")` (P0-adjacent: crash diagnostics)

```rust
// ✅ rapid: anyhow in library is acceptable
pub fn parse_config(input: &str) -> anyhow::Result<Config> {
    let value: Value = toml::from_str(input)
        .context("Failed to parse TOML config")?;
    Ok(Config::from(value))
}

// ✅ rapid: Clone freely, no hotpath optimization
fn process_users(users: &[User]) -> Vec<ProcessedUser> {
    users.iter().cloned()  // Clone is fine
        .map(|u| transform(u))
        .collect()
}
```

---

### `standard` — Default / Most Projects

**Goal**: Balance maintainability with development velocity.

**Enforced Priorities**: P0 (Safety) + P1 (Maintainability)

**Warning Level**: P2 (Compile Time) — Agent should warn but not block

| Rule | Standard Mode Behavior |
|------|----------------------|
| Error handling in libraries | `thiserror` required for library public API; `anyhow` only in application binary |
| `.clone()` usage | Allowed on cold paths; document if used on hot paths |
| State machine design | Type-driven state machine for core domain entities; `Option` fields acceptable for peripheral entities |
| Doc comments | Required on `pub` items; doc-tests encouraged but not mandatory |
| `#[non_exhaustive]` | Required on evolving public enums and structs |
| Sealed traits | Required for internal dispatch traits |
| Tracing instrumentation | Required on `pub async fn`; `skip` large fields |
| CI/CD checks | `cargo clippy` + `cargo test` + `cargo deny check` |
| Compile time warnings | Agent warns when generics cause >2x compile time increase |

**Mandatory in Standard**:
- All P0 rules (safety, no UB, SAFETY comments)
- All P1 rules (semantic naming, owned types in business logic, trait-based decoupling)
- Trade-off analysis in code comments for non-obvious decisions

---

### `strict` — Production Release / Safety-Critical / Open-Source Library

**Goal**: Maximum correctness, security, and long-term maintainability.

**Enforced Priorities**: P0 + P1 + P2 + P3 (all levels)

| Rule | Strict Mode Behavior |
|------|---------------------|
| Error handling | `thiserror` with rich context for all library code; no `anyhow` in library |
| `.clone()` usage | Every `.clone()` must be justified in comment; zero-copy preferred |
| State machine design | Type-driven state machine mandatory; impossible states must be unrepresentable |
| Doc comments & doc-tests | Required on all `pub` items; doc-tests must pass |
| `#[non_exhaustive]` | Required on all public types |
| Sealed traits | Required for all non-extension traits |
| Tracing instrumentation | Mandatory on all `pub async fn` + `pub fn` in service layer |
| CI/CD checks | Full suite: clippy (deny warnings), miri, cargo deny, fuzz, loom |
| Performance | All P3 optimizations require profiler evidence |
| Deviation | Any rule deviation requires `// DEVIATION: reason` annotation + review |

**Additional Requirements in Strict**:
- `cargo miri test` must pass for all unsafe code
- `cargo fuzz` targets for all external input parsers
- `cargo deny check` with no advisories allowed
- Property-based tests (`proptest`) for core algorithms
- No `unwrap()` or `expect()` in library code — use `Result` propagation

---

## Mode Selection Decision Tree

```
Is this a safety-critical system (medical, aerospace, financial)?
├─ Yes → strict
└─ No
   ├─ Is this an open-source library with external users?
   │  ├─ Yes → strict
   │  └─ No
   │     ├─ Is this a prototype / hackathon / unproven business model?
   │     │  ├─ Yes → rapid
   │     │  └─ No → standard
   │     └─ Is this approaching production release?
   │        ├─ Yes → strict (for final review pass)
   │        └─ No → standard
```

## Mode Transition Protocol

Projects typically progress: `rapid` → `standard` → `strict`

### Transition Checklist: rapid → standard

- [ ] Replace `anyhow` in library crates with `thiserror` error types
- [ ] Add `#[non_exhaustive]` to evolving public types
- [ ] Add `#[tracing::instrument]` to `pub async fn`
- [ ] Replace `bool` flags with state enums for core entities
- [ ] Add doc comments to all `pub` items
- [ ] Set up `cargo clippy` + `cargo deny check` in CI
- [ ] Review all `.clone()` calls on suspected hot paths

### Transition Checklist: standard → strict

- [ ] Convert core domain entities to type-driven state machines
- [ ] Add `cargo miri test` for all unsafe code
- [ ] Add fuzz targets for all external input parsers
- [ ] Add property-based tests for core algorithms
- [ ] Replace all `unwrap()`/`expect()` in library code with `Result`
- [ ] Justify every `.clone()` with comment
- [ ] Full `cargo deny check` with zero advisories
- [ ] Review all `// DEVIATION:` annotations

## Configuration

Set the mode in your project's SKILL invocation or environment:

```bash
# In SKILL argument
rust-architecture-guide --mode rapid

# Or via environment variable
RUST_SKILL_MODE=rapid
```

If no mode is specified, `standard` is assumed.

## Related

- [priority-pyramid.md](01-priority-pyramid.md) — Priority levels referenced by modes
- [progressive-architecture.md](03-progressive-architecture.md) — Lifecycle stage architecture
- [conflict-resolution.md](02-conflict-resolution.md) — How modes affect conflict resolution
- [review.md](27-review.md) — Review checklist adapts by mode
