---
title: "Toolchain & CI"
description: "Clippy, Miri, cargo deny, Workspace organization, and CI pipeline design"
category: "Tooling"
priority: "P0-P2"
applies_to: ["rapid", "standard", "strict"]
prerequisites: []
dependents: ["rust-systems-cloud-infra-guide/references/10-ci-lints.md"]
---

# Toolchain & CI

## Clippy Configuration

### CI Setup

```toml
# rust-toolchain.toml — Lock compiler version
[toolchain]
channel = "1.75.0"  # Prevents CI breaks from Clippy updates
```

```bash
# CI command
cargo clippy -- -D warnings
```

### Allowing Exceptions

```rust
// Must include explanatory comment
#[allow(clippy::too_many_arguments)] // Domain model requires many parameters
struct ComplexConfig { }
```

## Unsafe Guidelines (Strict Isolation)

### Requirements for Any `unsafe` Block (must satisfy before any unsafe block)

1. **Strictly encapsulated** in safe API wrapper
2. **SAFETY comment** explaining why memory safety is preserved
3. **Profiling evidence** that this is a bottleneck

```rust
fn unsafe_optimization(data: &[u8]) {
    // SAFETY: Bounds check performed on previous line via data.len() > 0
    // This memory operation preserves Rust's core invariants because...
    unsafe {
        *data.get_unchecked(0)
    }
}
```

### When to Use Unsafe

- FFI calls
- Extreme micro-optimizations eliminating bounds checks
- **Only after** profiling proves it's a bottleneck

**Rule**: Never use unsafe for convenience. Always preserve safe abstraction boundaries.

## Documentation Tests

### Requirements by Module Type

| Module Type | Doc-test Requirement |
|------------|---------------------|
| Public API of published crates | Must include executable doc-tests |
| Internal `pub` modules | Use `#[doc(hidden)]`, exempt from doc-tests |

```rust
/// Internal module, not for end users
#[doc(hidden)]
pub mod internal { }
```

**Why**: Avoids slowing down test suite execution.

## Cargo Workspace & Incremental Compilation Defense

### Pain Point

As projects grow, Rust compilation speed becomes a team efficiency killer. If all code is crammed into one Crate, changing one line can trigger minutes-long incremental compilation.

### Rule: Physical Isolation via Workspace

**Specification**: Must use Cargo Workspace. Split the system into logically clear independent Crates:

```toml
# Cargo.toml (workspace root)
[workspace]
members = [
    "crates/xxx-core",    # Pure business domain models & algorithms, no I/O
    "crates/xxx-proto",   # Protocol data structures & serialization
    "crates/xxx-infra",   # Database, cache, and other infrastructure
    "crates/xxx-api",     # Business routes & controllers
]
```

**Benefits**:
- Changing `xxx-api` only recompiles that crate and dependents, not `xxx-core`
- Parallel compilation across independent crates
- Clear dependency direction prevents circular coupling

### Rule: Workspace Dependency Inheritance

**Specification**: Use `workspace = true` to centralize dependency versions. Avoid version duplication across member crates.

```toml
# Cargo.toml (workspace root)
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
anyhow = "1"

[workspace.metadata]
rust-version = "1.85.0"
```

```toml
# crates/xxx-core/Cargo.toml
[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }
```

**Rust 2024 Note**: `default-features = false` in workspace inheritance is now rejected unless the workspace definition explicitly does not enable default features. Always be explicit in workspace definitions.

### Rule: Dependency Slimming via Feature Flags

**Specification**: Don't make users pay for features they don't need.

```toml
# crates/xxx-core/Cargo.toml
[features]
default = []
serde = ["dep:serde"]      # Only pull serde when needed
tokio = ["dep:tokio"]      # Only pull tokio when needed
full = ["serde", "tokio"]

[dependencies]
serde = { version = "1", optional = true }
tokio = { version = "1", optional = true }
```

**Architecture Benefits**:
- Reduces final binary size
- Dramatically shortens compile time for modules only depending on core logic
- Downstream crates opt-in only to features they need

## Dependency Risk Management: `cargo deny`

### Pain Point

Rust's ecosystem is thriving but prone to "dependency hell". A seemingly simple library may indirectly pull in 200 unrelated Crates.

### Rule: Supply Chain Security Audit

**Specification**: Must regularly use `cargo deny` to check the dependency tree.

```toml
# deny.toml
[advisories]
vulnerability = "deny"      # No known vulnerabilities
unmaintained = "warn"       # Warn on unmaintained crates

[licenses]
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause"]
deny = ["GPL-2.0", "GPL-3.0"]  # Prevent GPL contamination

[bans]
multiple-versions = "warn"  # Warn on duplicate versions (binary bloat)
```

```bash
# CI command
cargo deny check
```

**Monitoring Targets**:
- **Vulnerabilities**: No crates with known security advisories
- **License compliance**: No GPL or other non-compliant licenses contaminating the project
- **Duplicate versions**: Avoid same library at different versions causing binary bloat

## rustfmt Configuration

### Rule: Commit `rustfmt.toml` to Enforce Consistent Style

```toml
# rustfmt.toml
edition = "2021"
max_width = 100
use_small_heuristics = "Default"
fn_params_layout = "Tall"          # One param per line for long signatures
imports_granularity = "Crate"      # Merge imports at crate level
group_imports = "StdExternalCrate" # Group: std → external → crate
reorder_imports = true
reorder_impl_items = true
```

### CI Integration

```bash
# CI: fail if code is not formatted
cargo fmt -- --check
```

**Rule**: Never skip `cargo fmt --check` in CI. Unformatted code in main creates merge conflicts and review noise.

## CI Pipeline Design

### Minimum Viable CI

```yaml
# .github/workflows/ci.yml
jobs:
  check:
    steps:
      - run: cargo fmt -- --check           # Style consistency
      - run: cargo clippy -- -D warnings    # Lint enforcement
      - run: cargo test                     # Correctness
      - run: cargo deny check               # Supply chain security
```

### Production CI (Full Pipeline)

```yaml
jobs:
  check:
    steps:
      - run: cargo fmt -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all-features
      - run: cargo deny check
      - run: cargo miri test                # UB detection (unsafe projects)
  coverage:
    steps:
      - run: cargo tarpaulin --out Stdout   # Code coverage
  benchmark:
    steps:
      - run: cargo bench                    # Performance regression
```

### Rule: CI Must Be Fast (< 10 minutes)

- **Split jobs** for parallelism (fmt, clippy, test, deny)
- **Cache `~/.cargo` and `target/`** between runs
- **Skip heavy checks on PR** (miri, bench) — run on merge to main
- **Use `cargo check` instead of `cargo build`** where possible

## Trade-offs

**Strictness vs Flexibility**: `-D warnings` keeps code clean but may block legitimate exceptions.

**Unsafe encapsulation**: More boilerplate but preserves safe abstraction boundaries.

**Workspace granularity**: Too many tiny crates increase maintenance overhead; too few lose incremental compilation benefits.

**CI thoroughness vs speed**: Full miri + bench on every PR is thorough but slow. Run heavy checks on merge, light checks on PR.

## Related

- [error-handling.md](10-error-handling.md) — Error types and unsafe error handling
- [review.md](27-review.md) — Code review checklist including toolchain compliance
- [api-design.md](13-api-design.md) — API evolution and forward compatibility
- [advanced-testing.md](26-advanced-testing.md) — Miri and loom CI integration
