---
title: "Deviation Process — Formal Rule Exception Handling"
description: "Formal process for documenting and reviewing rule deviations with // DEVIATION: annotations"
category: "Process"
priority: "P0-P3"
applies_to: ["rapid", "standard", "strict"]
prerequisites: ["00-mode-guide.md", "01-priority-pyramid.md"]
dependents: []
---

# Deviation Process — Formal Rule Exception Handling

## Overview

Occasionally, breaking a rule is the correct engineering decision. Examples:
- Using `unsafe` to eliminate bounds checks in a profiled hot path
- Using `anyhow` in a library crate during rapid prototyping
- Skipping `#[non_exhaustive]` on an internal enum that will never evolve

A **deviation** is not a violation — it is a conscious, documented trade-off. This document defines the formal process for recording and reviewing deviations.

---

## Deviation Annotation

### Code Annotation Format

Every deviation MUST be annotated in code with a comment following this exact format:

```rust
// DEVIATION: [rule-id] - [reason]
// Context: [why this deviation is safe/necessary]
// Mode: [rapid|standard|strict]
// Review: [required|not-required|deferred]
```

### Examples

```rust
// DEVIATION: P1-error-layering - Using anyhow in library crate during prototyping
// Context: Business model unproven; this crate will be rewritten if validated
// Mode: rapid
// Review: not-required
pub fn parse_config(input: &str) -> anyhow::Result<Config> {
    // ...
}
```

```rust
// DEVIATION: P0-unsafe - Using unchecked access in profiled hot path
// Context: Flamegraph shows 15% of CPU time in bounds checks; index proven valid by loop invariant
// Mode: strict
// Review: required
fn process_batch(data: &[u8], indices: &[usize]) -> Vec<u8> {
    indices.iter().map(|&i| {
        // SAFETY: i is guaranteed in bounds by the caller's validation (see validate_indices)
        unsafe { *data.get_unchecked(i) }
    }).collect()
}
```

```rust
// DEVIATION: P1-no-clone - Cloning large struct on hot path
// Context: Profiling shows clone takes <0.1% of total time; refactoring to Arc would add complexity
// Mode: standard
// Review: deferred
fn transform_batch(items: &[LargeItem]) -> Vec<Output> {
    items.iter().cloned()  // DEVIATION: P1-no-clone - Clone acceptable per profiling
        .map(|item| item.transform())
        .collect()
}
```

---

## Rule ID Reference

Each rule in the priority pyramid has a unique identifier for deviation tracking:

### P0 Rules (Safety & Correctness)
| Rule ID | Description |
|---------|-------------|
| `P0-unsafe` | Unsafe code usage |
| `P0-data-race` | Potential data race |
| `P0-ffi-panic` | Panic crossing FFI boundary |
| `P0-ub` | Potential undefined behavior |

### P1 Rules (Maintainability)
| Rule ID | Description |
|---------|-------------|
| `P1-error-layering` | Error handling layer violation (anyhow in library) |
| `P1-lifetime-pollution` | Complex lifetime annotations in business logic |
| `P1-no-clone` | Unnecessary clone on hot path |
| `P1-state-machine` | Using bool/Option instead of type-driven state machine |
| `P1-doc-comments` | Missing doc comments on pub items |
| `P1-non-exhaustive` | Missing #[non_exhaustive] on evolving public type |

### P2 Rules (Compile Time)
| Rule ID | Description |
|---------|-------------|
| `P2-monomorphization` | Excessive generic monomorphization |
| `P2-proc-macro` | Procedural macro causing >2x compile time |
| `P2-workspace` | Missing workspace split for independent crates |

### P3 Rules (Runtime Performance)
| Rule ID | Description |
|---------|-------------|
| `P3-premature-opt` | Optimization without profiler evidence |
| `P3-unsafe-opt` | Unsafe optimization (bounds check elimination, SIMD) |
| `P3-allocator` | Custom allocator selection |

---

## Review Requirements by Mode

### `rapid` Mode
- Deviations from P0: **Always require review** (P0 is never relaxed)
- Deviations from P1/P2/P3: **Not required** (these levels are not enforced in rapid mode)

### `standard` Mode
- Deviations from P0: **Always require review**
- Deviations from P1: **Required** if the deviation affects public API or core domain logic
- Deviations from P2: **Not required** (P2 is warning-level in standard mode)
- Deviations from P3: **Not required** (P3 is not enforced in standard mode)

### `strict` Mode
- Deviations from any level (P0-P3): **Always require review**
- Deviations must be approved by a second review pass
- Deviations from P0 in strict mode require a **safety argument** in the Context field

---

## Deviation in Decision Summary

Every Decision Summary block (see SKILL.md Mandatory Output Contract) must include a Deviations section:

```markdown
## Decision Summary
- **Mode**: strict
- **Rules Applied**: P0-unsafe, P1-error-layering, P1-state-machine
- **Conflicts Resolved**: P3 > P1 (profiler shows 15% CPU in bounds checks)
- **Deviations**:
  - `P0-unsafe` at src/batch.rs:42 — bounds check elimination, SAFETY comment present
  - `P1-no-clone` at src/service.rs:78 — clone <0.1% per profiling
- **Trade-offs**: Safety wrapper adds 2ns per call; acceptable for 10K QPS service
```

---

## Deviation Audit

### Automated Detection

Use `grep` or `ripgrep` to find all deviations in the codebase:

```bash
# Find all deviations
rg "DEVIATION:" --line-number

# Find deviations requiring review
rg "DEVIATION:.*Review: required" --line-number

# Find P0 deviations (always critical)
rg "DEVIATION: P0-" --line-number

# Count deviations by rule
rg "DEVIATION:" --only-matching | sort | uniq -c | sort -rn
```

### Periodic Review

In `strict` mode, deviations should be reviewed:
1. **On every major version bump**: Re-evaluate all deviations
2. **On mode transition** (standard → strict): All deviations require fresh review
3. **Quarterly audit**: Check if deviations are still justified (profiling data may be stale)

### Deviation Removal Checklist

Before removing a deviation annotation:
- [ ] The original reason no longer applies
- [ ] The code can be refactored to comply with the rule
- [ ] Tests pass after removing the deviation
- [ ] No new deviations are introduced by the refactoring

---

## Anti-Patterns

### ❌ Silent Deviation
Breaking a rule without annotation. This is a **violation**, not a deviation.

```rust
// ❌ No annotation — this is a violation
fn process(data: &[u8], i: usize) -> u8 {
    unsafe { *data.get_unchecked(i) }
}
```

### ❌ Vague Deviation
Annotation without specific justification.

```rust
// ❌ Vague — no actionable context
// DEVIATION: P0-unsafe - needed for performance
unsafe { *data.get_unchecked(i) }
```

### ❌ Stale Deviation
Deviation that was justified at one point but is no longer valid (e.g., the hot path was refactored away). Stale deviations should be removed during periodic audit.

---

## Related

- [00-mode-guide.md](00-mode-guide.md) — Mode definitions that govern deviation review requirements
- [priority-pyramid.md](01-priority-pyramid.md) — Priority levels and rule definitions
- [conflict-resolution.md](02-conflict-resolution.md) — Conflict resolution when deviations are not appropriate
- [review.md](27-review.md) — Review checklist that checks for deviation annotations
- [glossary.md](05-glossary.md) — Terminology definitions
