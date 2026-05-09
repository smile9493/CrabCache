---
title: "Modern CI/CD for Cloud Infrastructure"
description: "IO_uring benchmarks in CI, deterministic seed testing, long-running soak tests, fuzzing for consensus protocols"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["10-ci-lints.md", "05-resilience.md"]
dependents: []
aligned_with: ["Rust CI/CD Best Practices 2025-2026", "io_uring-benchmark"]
---

# Modern CI/CD for Cloud Infrastructure

> **Core Philosophy — Infrastructure CI as Medical Monitoring (基础设施 CI 即医疗监控)**: Cloud infrastructure CI is not just about "code passes tests." It is continuous physiological monitoring of the system's performance, determinism, and resilience — the equivalent of an ICU monitor for distributed systems.

Vertical deepening of [`33-ci-modern.md`](../../rust-architecture-guide/references/33-ci-modern.md) for cloud infrastructure scenarios.

---

## 1. I/O Model Regression Benchmarks

### 1.1 io_uring vs epoll Benchmark in CI

**Rule**: Cloud infrastructure projects using io_uring must benchmark I/O model performance regression in CI.

```yaml
io-bench:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - run: cargo bench --bench io_model -- --save-baseline current
    - name: Compare with PR base
      run: |
        git fetch origin ${{ github.base_ref }}
        git checkout origin/${{ github.base_ref }}
        cargo bench --bench io_model -- --save-baseline base
        git checkout ${{ github.head_ref }}
        cargo bench --bench io_model -- --baseline base
    - name: Alert on regression
      run: |
        # Fail if >5% throughput loss or >10% latency increase
        python3 scripts/check_io_regression.py
```

### 1.2 I/O Model Selection Audit

```bash
# Audit: ensure no accidental mixed runtimes
cargo check --features io_uring
cargo check --no-default-features --features epoll
```

**Red Line**: Mixed tokio + io_uring runtime in production must be caught at CI level — not just documented.

---

## 2. Consensus Determinism Verification

### 2.1 Deterministic State Machine CI Gate

**Rule**: Any Raft/Paxos state machine must pass determinism CI — run same log through same code twice, verify identical state.

```rust
#[test]
fn test_state_machine_determinism() {
    let log = generate_test_log(SEED);

    // Run twice, compare byte-for-byte
    let state1 = StateMachine::new().apply_all(&log);
    let state2 = StateMachine::new().apply_all(&log);

    assert_eq!(
        bincode::serialize(&state1).unwrap(),
        bincode::serialize(&state2).unwrap(),
        "State machine is NOT deterministic"
    );
}
```

### 2.2 Non-Determinism Detection

```bash
# grep for forbidden constructs in consensus code
grep -r "Instant::now\|SystemTime::now\|rand::\|HashMap::new\|HashSet::new" \
    src/consensus/ && echo "ERROR: Non-deterministic code in consensus layer" && exit 1
```

---

## 3. Long-Running Soak Tests

### 3.1 Memory Leak Detection

**Rule**: Cloud infrastructure must pass 24-hour soak tests detecting memory leaks.

```yaml
soak:
  runs-on: ubuntu-latest
  timeout-minutes: 1440  # 24 hours
  steps:
    - uses: actions/checkout@v4
    - run: cargo build --release
    - run: |
        target/release/service &
        PID=$!
        # Monitor memory every 10 minutes
        for i in $(seq 1 144); do
          sleep 600
          RSS=$(ps -o rss= -p $PID | tr -d ' ')
          echo "Hour $((i/6)): ${RSS}KB"
          # Memory must not grow >2x initial over 24h
        done
        kill $PID
```

### 3.2 Connection Leak & FD Exhaustion

```bash
# Monitor file descriptors during soak
watch -n 60 'ls /proc/$PID/fd | wc -l'
# FD count must stabilize, not grow unbounded
```

---

## 4. Chaos Testing in CI

### 4.1 Network Partition Chaos

```rust
#[cfg(test)]
mod chaos_tests {
    use turmoil::Builder;

    #[test]
    fn raft_survives_partition() {
        let mut sim = Builder::new().build();

        // 3-node Raft cluster
        sim.host("n1", || async { RaftNode::new(1).run().await });
        sim.host("n2", || async { RaftNode::new(2).run().await });
        sim.host("n3", || async { RaftNode::new(3).run().await });

        // Inject partition: n1 ↔ n2 ↔ n3
        sim.partition("n1", "n2");
        // Partitions must converge when repaired
        sim.repair("n1", "n2");
        sim.run().unwrap();
    }
}
```

### 4.2 Disk Latency Injection

```rust
#[cfg(feature = "chaos")]
fn disk_op(data: &[u8]) -> io::Result<()> {
    if chaos::is_active("disk_slow") {
        let delay = chaos::rand_delay(1..100); // 1-100ms
        std::thread::sleep(delay);
    }
    fs::write(PATH, data)
}
```

---

## 5. Cloud-Specific CI Checklist

1. **I/O model benchmark** showing no >5% regression?
2. **Consensus determinism test** passes (same log → same state)?
3. **No non-deterministic constructs** in consensus code (`HashMap`, `Instant::now`, `rand`)?
4. **24-hour soak test** shows no memory leak (>2x growth)?
5. **File descriptor count** stable under sustained load?
6. **Network partition chaos** tests converge?
7. **`cargo-fuzz`** targets for all protocol parsers?
8. **Disk latency injection** handled gracefully?
9. **Cross-compilation** verified for target architectures (aarch64)?
10. **`cargo deny check`** with zero critical advisories?

## Related

- [10-ci-lints.md](10-ci-lints.md) — CI lint configuration
- [05-resilience.md](05-resilience.md) — Resilience patterns
- [04-consensus.md](04-consensus.md) — Consensus determinism
- [01-io-model.md](01-io-model.md) — I/O model selection
- [33-ci-modern.md](../../rust-architecture-guide/references/33-ci-modern.md) — General CI modern practices
