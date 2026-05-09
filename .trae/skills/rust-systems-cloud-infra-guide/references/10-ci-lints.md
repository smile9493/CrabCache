---
title: "CI Mandatory Lints"
description: "deny-level lints configuration for CI/CD pipelines and pre-commit hooks"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["rapid", "standard", "strict"]
prerequisites: ["rust-architecture-guide/references/17-toolchain.md", "09-code-style.md"]
dependents: []
---

# Skill: CI Mandatory Lints

## 👤 Profile

* **Domain**: All production Rust projects.
* **Environment**: GitHub Actions / GitLab CI / pre-commit hooks.
* **Philosophy**:
    * **Automate Specification Enforcement**: Convert specs to CI mandatory rules, avoid human oversight.
    * **Test Environment Relaxation**: Strict for business code, allow `unwrap` in tests.

---

## ⚔️ Core Directives

### Action 1: Enable deny-Level Lints
* **Scenario**: Top of `lib.rs` / `main.rs`.
* **Execution**:
    ```rust
    #![deny(clippy::await_holding_lock)]
    #![deny(clippy::await_holding_refcell_ref)]
    #![deny(clippy::large_stack_frames)]
    #![deny(clippy::undocumented_unsafe_blocks)]
    #![deny(clippy::unwrap_used)]
    #![deny(clippy::expect_used)]
    #![deny(clippy::todo)]
    #![deny(clippy::dbg_macro)]
    #![deny(unsafe_op_in_unsafe_fn)]
    ```

### Action 2: Test Environment Relaxation
* **Scenario**: Need to use `unwrap` in test code.
* **Execution**:
    ```rust
    #![cfg_attr(test, allow(clippy::unwrap_used))]
    #![cfg_attr(test, allow(clippy::expect_used))]
    ```

---

## 💻 Code Paradigms

### Paradigm A: Complete Configuration

```rust
// Top of lib.rs or main.rs

#![deny(clippy::await_holding_lock)]
#![deny(clippy::await_holding_refcell_ref)]
#![deny(clippy::large_stack_frames)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unimplemented)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(non_ascii_idents)]

#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]
```

### Paradigm B: Cargo.toml Configuration

```toml
[lints.clippy]
await_holding_lock = "deny"
await_holding_refcell_ref = "deny"
large_stack_frames = "deny"
undocumented_unsafe_blocks = "deny"
unwrap_used = "deny"
expect_used = "deny"
todo = "deny"
dbg_macro = "deny"
unimplemented = "deny"

[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
non_ascii_idents = "deny"
```

### Paradigm C: GitHub Actions

```yaml
name: CI

on: [push, pull_request]

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all
      
  miri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@miri
      - run: cargo miri test
```

### Paradigm D: pre-commit Hook

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: clippy
        name: clippy
        entry: cargo clippy --all-targets -- -D warnings
        language: system
        pass_filenames: false
      - id: fmt
        name: fmt
        entry: cargo fmt --check
        language: system
        pass_filenames: false
```

---

## 📋 Lint Explanation

| Lint | Problem | Fix |
|------|---------|-----|
| `await_holding_lock` | Sync lock across `.await` | Move data out or use async Mutex |
| `await_holding_refcell_ref` | RefCell borrow across `.await` | Clone and drop before `.await` |
| `large_stack_frames` | Large stack allocation | Use `vec!` for heap |
| `undocumented_unsafe_blocks` | Missing `SAFETY` comment | Add `// SAFETY:` above block |
| `unwrap_used` | May panic | Use `?` or `ok_or` |
| `unsafe_op_in_unsafe_fn` | Implicit unsafe in unsafe fn | Wrap with `unsafe { }` + SAFETY |
