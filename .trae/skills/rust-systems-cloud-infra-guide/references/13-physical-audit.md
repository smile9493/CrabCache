---
title: "Physical Feasibility Audit for Cloud Deployments"
description: "Mandatory pre-design audit for cloud infrastructure: container memory limits, network latency budgets, NUMA topology, I/O cost prediction"
category: "Architecture"
priority: "P0"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/32-physical-audit.md"]
dependents: []
---

# Physical Feasibility Audit for Cloud Deployments

> **Prerequisites**: This document extends the generic physical audit from [`rust-architecture-guide/references/32-physical-audit.md`](../../rust-architecture-guide/references/32-physical-audit.md) for cloud infrastructure environments.

> **Philosophy**: Mechanical Sympathy. The cloud is not an abstract machine — it is a physical network of containers, NUMA nodes, and shared resources. Design must acknowledge physical constraints.

---

## 1. Cloud-Specific Audit Additions

The generic audit covers I/O budget, memory ceiling, and concurrency cost. For cloud deployments, add:

### 1.1 Container Memory Limit Audit

**Question**: What is the cgroup memory hard limit, and where is the RSS inflection point?

**Audit Steps**:
1. **Identify container limits**: cgroup memory limit (e.g., 512MB), swap (often disabled)
2. **Estimate per-instance baseline**:
   - Rust binary + shared libraries: ~10-50MB
   - Global allocator initial reservation: ~2-10MB
   - Thread stack reserve: `thread_count × stack_size` (default 2MB/thread)
3. **Calculate steady-state under load**:
   - Connection buffers: `concurrent_connections × buffer_size`
   - Request arenas: `concurrent_requests × arena_size`
   - Cache: configured max size
4. **Red Line**: If steady-state exceeds 70% of container limit, add memory-based circuit breaker.

### 1.2 Network Latency Budget Audit

**Question**: Can the service meet its SLO given real-world network variance?

**Audit Steps**:
1. **Identify all network hops**: Client → LB → Service → Downstream → Database
2. **Estimate per-hop latency distribution**:
   | Hop | p50 | p99 | Notes |
   |-----|-----|-----|-------|
   | Client → LB | 1ms | 10ms | Cross-region adds 50-200ms |
   | LB → Service | 0.1ms | 2ms | Same VPC |
   | Service → Downstream | 5ms | 50ms | Cross-service |
   | Service → Database | 1ms | 20ms | Connection pool dependent |
3. **Calculate total latency**: Sum of all hops (serial) or max of parallel paths
4. **Red Line**: If p99 latency exceeds SLO, add timeout + fallback at each hop.

### 1.3 NUMA Topology Audit

**Question**: Is the service NUMA-aware, or will cross-node memory access degrade performance?

**Audit Steps**:
1. **Identify deployment topology**: Bare metal with multiple NUMA nodes, or VMs (single NUMA domain)
2. **For bare metal NUMA**:
   - Bind threads to NUMA-local CPUs (`numactl`, `sched_setaffinity`)
   - Allocate memory on local NUMA node
   - Estimate cross-node latency penalty: ~2x local access
3. **For VMs/containers**: NUMA awareness is typically not needed (hypervisor handles placement)

---

## 2. Cloud-Specific Red Line Values

| Metric | Threshold | Required Action |
|--------|-----------|----------------|
| RSS > 70% of container limit | Add memory circuit breaker | Reject requests, evict cold cache |
| p99 latency > SLO | Add timeout + fallback | Per-hop timeout, cached response |
| Cross-NUMA memory access > 30% | NUMA-aware binding | Thread pinning, local allocation |
| Connection pool exhaustion | Bounded pool + backpressure | Queue + reject, never unbounded |
| Disk I/O wait > 20% of CPU | Async I/O + batching | io_uring, splice, sendfile |

---

## 3. Cloud Audit Report Addendum

Append to the generic audit report:

```markdown
## Cloud Deployment Audit

### Container Limits
| Resource | Limit | Steady-State | Peak | Safety Margin |
|----------|-------|-------------|------|---------------|
| Memory | 512MB | 200MB | 350MB | 31% ✅ |
| CPU | 4 cores | 1.5 cores | 3.2 cores | 20% ⚠️ |

### Network Latency Budget
| Path | p50 | p99 | SLO | Within SLO? |
|------|-----|-----|-----|-------------|
| Client → Service | 5ms | 45ms | 100ms | ✅ |
| Service → Database | 2ms | 25ms | 50ms | ✅ |
| Service → Cache | 0.5ms | 5ms | 10ms | ✅ |
| **Total p99** | | **75ms** | **100ms** | **⚠️ threshold** |

### NUMA Topology
| Node | CPU Cores | Local Memory | Threads Bound | Cross-Node Access |
|------|-----------|-------------|---------------|-------------------|
| NUMA 0 | 16 | 32GB | 8 | 5% ✅ |
| NUMA 1 | 16 | 32GB | 8 | 3% ✅ |
```

---

## 4. Philosophy: The Cloud is Physical

The cloud is not an abstraction — it is a physical network of machines, cables, and shared resources. Designing cloud infrastructure without acknowledging physical constraints is like building a bridge without knowing the river's depth.

This audit forces the architect to **feel the hardware** — to understand that every container limit, every network hop, every NUMA boundary has a real, measurable cost. This is **Mechanical Sympathy**: software that aligns with hardware reality, not fights it.
