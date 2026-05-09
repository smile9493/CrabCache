---
title: "Modern CI/CD & Development Infrastructure"
description: "Modern CI/CD pipeline: cargo-mutants, kani, turmoil, deterministic RNG, rust-toolchain.toml, PGO/BOLT in CI"
category: "Tooling"
priority: "P1-P2"
applies_to: ["standard", "strict"]
prerequisites: ["17-toolchain.md"]
dependents: ["26-advanced-testing.md"]
aligned_with: ["Rust CI/CD Best Practices 2025-2026", "Safety-Critical Rust Coding Guidelines"]
---

# Modern CI/CD & Development Infrastructure

> **Core Philosophy — Reproducible Determinism (可重现的确定性)**: CI is not a "nice-to-have check" — it is the physical gatekeeper standing between chaos and production. Every CI pipeline must produce the same result from the same commit, on any machine, at any time.

---

## 1. Toolchain Pinning: rust-toolchain.toml

**Red Line**: All projects must pin the Rust toolchain version. Unpinned toolchains produce different CI results on different days.

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"          # Pin to specific stable version
components = ["rustfmt", "clippy", "miri"]
targets = ["wasm32-unknown-unknown"]
profile = "default"
```

```toml
# For nightly-dependent projects (fuzzing, miri)
[toolchain]
channel = "nightly-2026-01-15"  # Pin specific nightly
components = ["rustfmt", "clippy", "miri", "rust-src"]
```

**Rationale**: Without pinning, CI passes today and fails tomorrow when Clippy adds new lints or rustc changes behavior. This is a "Heisenbug at CI scale."

---

## 2. Deterministic Testing

### 2.1 Deterministic RNG Seeds

**Red Line**: Tests must use deterministic seeds. Non-deterministic tests produce non-reproducible failures.

```rust
use rand::{SeedableRng, rngs::StdRng};

#[test]
fn test_with_fixed_seed() {
    let mut rng = StdRng::seed_from_u64(0xDEAD_BEEF_CAFE_BABE);
    // Deterministic — same results on every CI run
}
```

```rust
// CI environment variable override for exploration
fn get_test_seed() -> u64 {
    std::env::var("TEST_RNG_SEED")
        .ok()
        .and_then(|s| u64::from_str_radix(&s, 16).ok())
        .unwrap_or(0xCAFE_BABE_DEAD_BEEF)
}
```

### 2.2 No Timing-Based Tests

**Prohibited**: Tests that depend on wall-clock time (`std::thread::sleep`, `tokio::time::sleep` thresholds).

**Alternative**: Use condition waits with explicit signal channels:

```rust
// ❌ Forbidden: timing-dependent test
#[tokio::test]
async fn test_timeout() {
    let result = tokio::time::timeout(Duration::from_millis(100), slow_operation()).await;
    assert!(result.is_err());
}

// ✅ Required: deterministic condition wait
#[tokio::test]
async fn test_condition() {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async {
        slow_operation().await;
        tx.send(()).unwrap();
    });
    // Wait for explicit completion signal, not wall-clock
    tokio::time::timeout(Duration::from_secs(10), rx).await.unwrap().unwrap();
}
```

---

## 3. Full CI Pipeline Architecture

### 3.1 Standard Mode Pipeline

```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show         # Uses rust-toolchain.toml
      - run: cargo check --workspace --all-features
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test --workspace
      - run: cargo deny check

  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: cargo install cargo-fuzz
      - run: for target in fuzz/fuzz_targets/*.rs; do cargo fuzz run "$(basename $target .rs)" -- -max_total_time=120; done
```

### 3.2 Strict Mode Pipeline

```yaml
  miri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: cargo miri setup
      - run: cargo miri test --workspace

  loom:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: RUSTFLAGS="--cfg loom" cargo test --test loom_tests

  kani:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install kani-verifier && cargo kani setup
      - run: cargo kani --harness safety_proofs

  mutants:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install cargo-mutants
      - run: cargo mutants --timeout 600 --in-place -- --test-threads=4

  turmoil:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --tests integration::turmoil
```

---

## 4. Binary Analysis Gate

### 4.1 Size Budget Enforcement

```toml
# .twiggy.toml or clippy.toml
[checks]
max-binary-size = "5MB"       # Hard limit in strict mode
max-wasm-size = "500KB"       # For wasm32 targets
max-monomorphization-bloat = "2x"  # Generic bloat compared to non-generic baseline
```

```bash
# Check binary size in CI
cargo build --release
BINARY_SIZE=$(stat -c%s target/release/app)
if [ $BINARY_SIZE -gt 5242880 ]; then
    echo "ERROR: Binary exceeds 5MB budget ($BINARY_SIZE bytes)"
    exit 1
fi
```

### 4.2 Dependency Audit

```bash
cargo deny check        # License + security advisory check
cargo audit             # Known vulnerability check
cargo outdated -R       # Dependency freshness report (non-blocking)
cargo tree -d           # Duplicate dependency detection
```

---

## 5. Comprehensive Agent CI Checklist

1. **toolchain pinned** via `rust-toolchain.toml`?
2. **`cargo fmt --check`** in pipeline?
3. **`cargo clippy -- -D warnings`** (strict: + `clippy::pedantic`)?
4. **`cargo deny check`** with zero advisories allowed?
5. **`cargo test --workspace`** passing?
6. **Deterministic RNG seeds** in all randomized tests?
7. **No timing-based tests** — condition waits only?
8. **`cargo miri test`** for all unsafe code (strict)?
9. **`cargo-fuzz`** targets for all parsers (strict)?
10. **`kani` proofs** for core safety invariants (strict)?
11. **`cargo mutants`** zero surviving mutants (strict)?
12. **Binary size budget** enforced (strict)?
13. **`cargo outdated -R`** report reviewed (standard, non-blocking)?

## Related

- [17-toolchain.md](17-toolchain.md) — Foundational toolchain configuration
- [26-advanced-testing.md](26-advanced-testing.md) — kani, cargo-mutants, turmoil usage
- [25-performance-tuning.md](25-performance-tuning.md) — PGO/BOLT in CI
