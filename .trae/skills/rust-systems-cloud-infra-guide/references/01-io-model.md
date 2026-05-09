---
title: "I/O Model & Zero-Copy Architecture"
description: "Tokio epoll vs io_uring vs monoio decision tree for high-throughput scenarios"
category: "Infrastructure"
priority: "P1-P3"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/12-async-internals.md", "rust-architecture-guide/references/09-data-architecture.md"]
dependents: []
---

# I/O Model & Zero-Copy Architecture

> **📚 Prerequisites**: This document assumes understanding of basic async Rust from [`12-async-internals.md`](../../rust-architecture-guide/references/12-async-internals.md) and zero-copy principles from [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md) §1 (Zero-Copy Parsing).
> 
> **🔺 Deepening Direction**: Selecting between Tokio epoll, io_uring, and monoio for high-throughput scenarios (10GbE+, storage engines, gateways) with zero-copy pipeline optimization.
> 
> **📋 Document Profile**:
> - **Domain**: High-throughput gateways, storage engines, message queues, database WAL.
> - **Environment**: Linux kernel 5.1+ (io_uring support), 10GbE+ network.
> - **Mode**: `standard` (awareness) → `strict` (mandatory selection)
> - **Prerequisites**: [`12-async-internals.md`](../../rust-architecture-guide/references/12-async-internals.md), [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md)

## Philosophy

* **Maximize Kernel I/O Stack**: Eliminate syscall overhead, achieve true async kernel handoff.
* **Zero-Copy**: Data should flow, not be copied.

---

## ⚔️ Core Directives

### Action 1: io_uring vs epoll/kqueue Decision
* **Scenario**: Choose async I/O backend matching workload characteristics.
* **Decision Tree**:
    ```
    Primary characteristics?
         │
    ┌────┴────┐
    │         │
    Many      Many short
    TCP       I/O ops
    conns     (open/read/write)
    │         │
    ▼         ▼
    tokio +   io_uring
    epoll     runtime
              │
         ┌────┴────┐
         │         │
         Disk      Network
         intensive intensive
         │         │
         ▼         ▼
    tokio-uring  monoio
    ```
* **Red Line**: If mixing `tokio` and `io_uring`, must unify runtime across entire pipeline.

### Action 2: Kernel-Level Zero-Copy
* **Scenario**: Proxy gateway, storage transport, send data from disk directly to socket.
* **Execution**: Use `splice`, `sendfile`, `copy_file_range` (via `rustix`).

### Action 3: User-Space Buffer Sharing
* **Scenario**: Pass data blocks across Tasks without cloning.
* **Execution**: Globally use `bytes::Bytes` (refcount, O(1) clone).

### Action 4: Direct I/O & Page Cache Bypass
* **Scenario**: Custom storage engine with its own buffer pool.
* **Red Line**: Must use `O_DIRECT` to bypass Page Cache, avoid double caching.
* **Alignment**: Buffer address, File Offset, Length must be sector-aligned (512B/4KB).

---

## 💻 Code Paradigms

### Paradigm A: Kernel-Level Zero-Copy (splice)

```rust
use nix::fcntl::{splice, SpliceFFlags};
use std::os::unix::io::AsRawFd;

fn zero_copy_pipe_to_socket(
    pipe_in: impl AsRawFd,
    socket_out: impl AsRawFd,
    len: usize,
) -> io::Result<usize> {
    let transferred = splice(
        pipe_in.as_raw_fd(),
        None,
        socket_out.as_raw_fd(),
        None,
        len,
        SpliceFFlags::empty(),
    )?;
    Ok(transferred as usize)
}
```

### Paradigm B: User-Space Buffer Sharing (bytes::Bytes)

```rust
use bytes::{Bytes, BytesMut, BufMut};

struct ZeroCopyPipeline {
    buffer_pool: Vec<BytesMut>,
}

impl ZeroCopyPipeline {
    fn process_chunk(&mut self, data: &[u8]) -> Bytes {
        let mut buf = BytesMut::with_capacity(data.len());
        buf.put_slice(data);
        buf.freeze()  // O(1) clone via refcount
    }
}
```

### Paradigm C: Direct I/O Aligned Memory

```rust
use std::alloc::{alloc, dealloc, Layout};

struct AlignedBuffer {
    ptr: *mut u8,
    layout: Layout,
    size: usize,
}

impl AlignedBuffer {
    fn new(size: usize, alignment: usize) -> Self {
        let layout = Layout::from_size_align(size, alignment).unwrap();
        let ptr = unsafe { alloc(layout) };
        Self { ptr, layout, size }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) };
    }
}
```

---

## 📊 Runtime Comparison

| Runtime | I/O Backend | Use Case | Key Features |
|---------|-------------|----------|--------------|
| tokio | epoll/kqueue | General async | Rich ecosystem, stable |
| monoio | io_uring | High-throughput I/O | Pure io_uring, zero syscall |
| glommio | io_uring | Storage systems | Priority scheduling |
| tokio-uring | io_uring | Mixed scenarios | Compatible with tokio |
