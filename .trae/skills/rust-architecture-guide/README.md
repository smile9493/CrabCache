# Rust Architecture Guide

**Universal Rust Engineering Decision Constitution** — Covering architecture design, idiomatic coding style, metaprogramming, FFI interop, performance tuning, and advanced quality assurance for all Rust projects.

English | [简体中文](README.zh-CN.md)

## Overview

This guide is the **constitutional foundation** for AI coding assistants, providing:

- **Four-level priority system** (P0 Safety → P1 Maintainability → P2 Compile Time → P3 Performance)
- **Three execution modes** (rapid / standard / strict)
- **Type-driven architecture** (state machines, Newtype, zero-cost abstraction boundaries)
- **Ownership layering strategy** (business layer Owned, hotpath layer zero-copy)
- **Error handling layering** (library-level `thiserror`, application-level `anyhow`)
- **Jeet Kune Do coding style** (Intercepting Boilerplate, Economy of Motion, Hardware Sympathy)
- **Agent self-check list** + Decision Summary output contract

## Core Philosophy

> **Pursue excellence at system boundaries and hot paths; release mental load for internal flows and cold paths.**

### Priority Pyramid

| Priority | Focus | Rule |
|----------|-------|------|
| **P0** | Safety & Correctness | Memory safety, data consistency — non-negotiable |
| **P1** | Maintainability | Readability, local complexity control — default pursuit |
| **P2** | Compile Time | Build speed, CI/CD efficiency — measure then decide |
| **P3** | Runtime Performance | Only for proven bottlenecks — requires Profiler evidence |

### Dialectical Materialism

Engineering decisions are resolved through contradiction:
- **Unity of Opposites**: `unsafe` and safe are not enemies — `unsafe` is the material foundation of safe abstractions
- **Quantitative to Qualitative Change**: MVP `Option<bool>` flags accumulate, then must undergo qualitative change into Enum state machines
- **Negation of Negation**: Errors are not endpoints — panic → catch → graceful degradation, each negation reaches higher resilience

### Jeet Kune Do Coding Philosophy

| Principle | Description |
|-----------|-------------|
| **Intercepting Boilerplate** | If logic can be expressed in 1 line of pattern matching, never use 5 lines of nesting |
| **Economy of Motion** | Every line of code should point directly to intent. Eliminate redundant intermediate variables and implicit copies |
| **Hardware Sympathy** | Leverage iterators and zero-copy types, align with the compiler's inline optimization |

### Execution Modes

| Mode | Enforce | Trade-off |
|------|---------|-----------|
| `rapid` | P0 only | Unlimited `.clone()`, `anyhow` in libraries, no doc-tests |
| `standard` | P0 + P1 | Default for most projects |
| `strict` | P0–P3 | All deviations require formal `// DEVIATION:` annotation |

## Document Index

### Execution & Strategy (7)

| File | Topic |
|------|-------|
| `references/00-mode-guide.md` | Execution modes — rapid / standard / strict definitions and transitions |
| `references/01-priority-pyramid.md` | Four-level priority pyramid |
| `references/02-conflict-resolution.md` | Typical conflicts and resolutions |
| `references/03-progressive-architecture.md` | MVP → Production progressive architecture |
| `references/04-trade-offs.md` | Trade-off decision analysis framework |
| `references/05-glossary.md` | Centralized terminology glossary |
| `references/06-deviation-process.md` | Deviation process (`// DEVIATION:` annotation) |

### Architecture Patterns (14)

| File | Topic |
|------|-------|
| `references/07-state-machine.md` | Type-driven state machine design |
| `references/08-newtype.md` | Newtype pattern and type-safe IDs |
| `references/09-data-architecture.md` | Ownership, cloning, memory layout |
| `references/10-error-handling.md` | Library-level `thiserror` vs application-level `anyhow` |
| `references/11-concurrency.md` | Concurrency: channels, locks, RwLock, parking_lot, deadlock prevention |
| `references/12-async-internals.md` | Async internals: Future, Pin/Unpin, select!/join!, cancellation safety |
| `references/13-api-design.md` | Public API: `#[non_exhaustive]`, Sealed Trait, `#[deprecated]` |
| `references/14-metaprogramming.md` | Intercepting Boilerplate: declarative macros, procedural macros, const fn, const generics |
| `references/15-ffi-interop.md` | The Defense Wall: three-layer isolation, opaque pointers, panic containment, repr(C) |
| `references/16-observability.md` | Tracing, Metrics, Panic Hook, Coredump |
| `references/17-toolchain.md` | CI, Clippy, rustfmt, Workspace, Feature Flags, cargo deny |
| `references/30-memory-layout.md` | **Memory Layout Transparency**: struct padding audit, repr(C) mandate, cache-friendly design, alignment control |
| `references/31-breakwater-pattern.md` | **Breakwater Architecture**: Facade/Core layered pattern, boundary interception protocol, type contraction |
| `references/32-physical-audit.md` | **Physical Feasibility Audit**: I/O budget, memory ceiling, concurrency true cost, mandatory pre-design audit |

### Idiomatic Style (7)

| File | Topic |
|------|-------|
| `references/18-control-flow.md` | `let else`, `matches!`, intercepting deep nesting |
| `references/19-iterators.md` | Iterator chains, `filter_map`, flowing force |
| `references/20-traits.md` | `From` vs `Into`, `Default`, Hardware Sympathy |
| `references/21-errors.md` | `unwrap_or_else`, `map_err`, `and_then` |
| `references/22-data-struct.md` | Field shorthand, type stuttering |
| `references/23-borrowing.md` | `AsRef`, `Cow`, memory economy |
| `references/24-refactor.md` | Agent Self-Check List, Reduction Directive |

### Performance & QA (4)

| File | Topic |
|------|-------|
| `references/25-performance-tuning.md` | Mechanical Sympathy: memory, cache, lock-free, SIMD, BCE, prefetching |
| `references/26-advanced-testing.md` | Machine vs Machine: proptest, fuzzing, loom, Miri, turmoil, defense report |
| `references/27-review.md` | Comprehensive review checklist |
| `references/28-usage-examples.md` | Real-world usage examples |

## Relationship

- **Standalone use**: Applicable to all Rust projects (web services, CLI tools, libraries, etc.)
- **Combined use**: `rust-systems-cloud-infra-guide` provides vertical deepening for cloud infrastructure scenarios

## License

MIT
