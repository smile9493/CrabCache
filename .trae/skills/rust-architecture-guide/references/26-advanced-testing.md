---
title: "Advanced Testing & Quality Assurance: Machine vs Machine"
description: "proptest, fuzzing, loom, miri, turmoil, kani, cargo-mutants for machine-level verification"
category: "Quality"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["17-toolchain.md", "33-ci-modern.md"]
dependents: ["rust-systems-cloud-infra-guide/references/06-observability.md"]
aligned_with: ["Rust CI/CD Best Practices 2025-2026", "Safety-Critical Rust Coding Guidelines"]
---

# Advanced Testing & Quality Assurance: Machine vs Machine

> **Core Philosophy — Machine vs Machine, Intercepting the Edge (边缘拦截, Bug Prevention), Deterministic Logic**: Human-written example-based tests only cover known territory. Against the extreme complexity of concurrency and edge cases in cloud infrastructure, we must unleash machine computation — algorithmically generating millions of adversarial inputs to comprehensively encircle bugs at the logical level.

---

## 1. Property-based Testing: Intercepting Logical Flaws

> **From "concrete examples" to "universal physical laws".**

### 1.1 Law Proof Over Example Accumulation

**Rule**: For protocol encode/decode, complex mathematical algorithms (e.g., Raft log index computation), and sorting logic, **must** use `proptest`.

**Verify invariants**: Symmetry (`decode(encode(x)) == x`), idempotency, and monotonicity.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn encode_decode_roundtrip(data in any::<Packet>()) {
        let encoded = encode(&data);
        let decoded = decode(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }
}
```

### 1.2 Minimal Shrinking

**Rule**: When a property test fails, the Agent **must** analyze the minimized input provided by `proptest` and store it as a permanent regression test case in the unit test suite.

```rust
// After failure, proptest shrinks to minimal case
// Save as regression:
#[test]
fn regression_vec_length_5() {
    let vec = vec![0, 0, 0, 0, 0];
    assert!(process_vec(&vec).is_ok());
}
```

---

## 2. Fuzzing: Resilience Under Extreme Adversarial Input

> **Introduce genetic mutation and evolutionary selection, searching for crash points in chaos.**

### 2.1 External Input Defense Net

**Rule**: All parsers directly exposed to network or disk **must** have `cargo-fuzz` targets. Fuzz targets must be pure computation — strip all external I/O. Leverage LLVM coverage-guided mutation for billion-iteration assaults in nightly CI.

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_http_request(data);
});
```

### 2.2 Zero Panic Criterion

**Rule**: Parser-layer code facing any malformed data must return `Err` or silently discard — **absolutely prohibit** `panic!` or `unwrap()` that could trigger OOM.

```rust
// ❌ Forbidden: panic on malformed input
fn parse_header(data: &[u8]) -> Header {
    let len = data[0] as usize; // Could panic if data is empty
    Header { len, .. }
}

// ✅ Required: graceful error handling
fn parse_header(data: &[u8]) -> Result<Header, ParseError> {
    let len = *data.first().ok_or(ParseError::TooShort)?;
    Ok(Header { len, .. })
}
```

---

## 3. Concurrency Model Checking: Intercepting One-in-a-Million Deadlocks

> **Eliminate the randomness of thread scheduling, achieve exhaustive deterministic exploration of concurrent state space.**

### 3.1 Deterministic Scheduling Verification

**Rule**: Any hand-written lock-free data structure or atomic synchronization logic **must** use `loom` for model checking tests.

```rust
#[cfg(loom)]
#[test]
fn test_lock_free_queue_concurrent_push() {
    loom::model(|| {
        let queue = LockFreeQueue::new();

        let t1 = loom::thread::spawn({
            let q = queue.clone();
            move || q.push(1)
        });

        let t2 = loom::thread::spawn({
            let q = queue.clone();
            move || q.push(2)
        });

        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(queue.len(), 2);
    });
}
```

### 3.2 Combinatorial Explosion Control

**Rule**: `loom` tests must maintain extremely short execution paths — 2-3 threads, 1-2 conflicting operations per thread — to manage the exponential growth of concurrent state space.

---

## 4. Dynamic Analysis: Intercepting Memory Sins

> **Before LLVM optimizes your code, use an interpreter to scrutinize every memory contract.**

### 4.1 Unsafe Life-or-Death Line

**Rule**: Any code containing `unsafe` **must** pass `cargo miri test` in CI.

**Capture targets**: Data races, strict aliasing violations, uninitialized memory reads.

```bash
rustup toolchain install nightly --component miri
cargo miri setup
cargo +nightly miri test
```

### 4.2 Address & Leak Detection

**Rule**: In integration testing, enable ASAN (Address Sanitizer) and LSAN (Leak Sanitizer) to ensure FFI boundaries have no heap buffer overflow or memory leaks.

```bash
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --target x86_64-unknown-linux-gnu
```

---

## 5. Simulation & Chaos: Thriving in Hostile Environments

> **Simulate the collapse of physical laws in a virtual world.**

### 5.1 Deterministic Network Simulation

**Rule**: For distributed consensus (Raft) or distributed databases, evaluate introducing `turmoil` or `madsim`.

**Simulation scenarios**: Multi-node network partitions, clock drift, packet loss, disk I/O latency jitter — ensuring system convergence under extreme physical failures.

```rust
use turmoil::Builder;

#[test]
fn test_raft_under_partition() -> Result {
    let mut sim = Builder::new().build();
    sim.host("node-1", || async { Raft::new("node-1").run().await });
    sim.host("node-2", || async { Raft::new("node-2").run().await });
    sim.partition("node-1", "node-2");
    sim.repair("node-1", "node-2");
    Ok(())
}
```

### 5.2 Fault Injection

**Rule**: Embed fault injection points in I/O adapter layers, triggered by environment variables for random disk write failures or network disconnections.

```rust
fn write_to_disk(data: &[u8]) -> io::Result<()> {
    if std::env::var("FAULT_INJECTION_DISK_FAIL").is_ok() {
        if rand::thread_rng().gen_bool(0.1) {
            return Err(io::Error::new(io::ErrorKind::Other, "injected disk failure"));
        }
    }
    fs::write(PATH, data)
}
```

---

**Defense Report Directive**: When delivering any infrastructure-grade component, the Agent must attach a Defense Report listing which machine verification methods were applied, and declaring that the system maintained P0-level memory safety and state consistency under hundreds of millions of mutation iterations.

---

## 6. Formal Verification: Kani Model Checker

> **Prove properties for all possible inputs, not just tested ones.**

### 6.1 When to Use Kani

**Rule**: For safety-critical algorithms (cryptographic invariants, consensus safety properties, memory allocator correctness), use `kani` to **prove** (not test) that assertions hold for all possible inputs.

**Kani excels at**:
- Bounded model checking for Rust code
- Proving `assert!` conditions are never violated
- Verifying integer overflow safety and arithmetic correctness
- Catching logic errors that property tests might miss

```rust
#[kani::proof]
fn test_allocator_invariant() {
    let size: u64 = kani::any();
    kani::assume(size > 0 && size < 1024 * 1024); // 1 byte to 1 MB

    let layout = Layout::from_size_align(size as usize, 8).unwrap();
    let ptr = unsafe { alloc(layout) };
    assert!(!ptr.is_null(), "Allocator must not return null for valid layout");
    // Not reached if assertion fails — kani reports proof failure
}
```

```bash
cargo kani --harness allocator_invariant
```

### 6.2 Kani vs Other Tools

| Tool | Verifies | Exhaustive? | Speed |
|------|----------|-------------|-------|
| `proptest` | Property holds for sampled inputs | No (sampling) | Fast |
| `cargo-fuzz` | No crash on mutated inputs | No (coverage-guided) | Medium |
| `kani` | Property holds for ALL inputs within bounds | Yes (bounded) | Slow (CBD) |
| `miri` | No UB for concrete test inputs | No (concrete) | Very slow |

**Rule**: Use kani for **core invariants** (consensus safety, cryptographic properties). Use proptest/fuzz for **defense in depth** (parser robustness, protocol conformance).

---

## 7. Mutation Testing: cargo-mutants

> **Test your tests — find gaps in your test coverage.**

### 7.1 When to Use

**Rule**: For `strict` mode libraries and infrastructure crates, run `cargo mutants` to identify untested code paths.

**What it does**: Introduces small mutations (e.g., `>` → `>=`, `+` → `-`, removing function calls) and checks if tests catch them. Surviving mutants = untested code.

```bash
cargo mutants --in-place
# Lists surviving mutants — each is a test coverage gap
```

```rust
// mutation: `>` becomes `>=` — if no test fails, boundary is untested
pub fn is_overdrawn(balance: i64) -> bool {
    balance > 0  // What if this became `balance >= 0`? Would tests catch it?
}
```

### 7.2 Integration into CI

```yaml
# .github/workflows/mutation.yml
- name: Mutation testing
  run: cargo mutants --timeout 300 --in-place -- --test-threads=4
```

**Rule**: Fix or document surviving mutants. A surviving mutant is either:
1. Dead code (remove it)
2. Missing test coverage (add a test)
3. Equivalent mutant (document why the mutation is semantically equivalent)

## 8. Agent QA Checklist

1. **Parsing logic protected by `cargo-fuzz`?**
2. **Complex algorithms proven via `proptest` symmetry or idempotency?**
3. **Hand-written atomics verified in `loom`?**
4. **`unsafe` code verified UB-free by `Miri`?**
5. **Failed random cases converted to deterministic regression tests?**
6. **Core safety invariants formally verified by `kani`?**
7. **Test coverage gaps identified by `cargo mutants`?**
8. **Distributed consensus verified under network chaos by `turmoil`?**
