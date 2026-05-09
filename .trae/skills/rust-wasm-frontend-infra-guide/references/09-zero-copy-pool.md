---
title: "Zero-Copy Resource Pool"
description: "Physical infrastructure for zero-copy transfer of variable-length data (strings, blobs, paths) between JS and WebAssembly via continuous linear memory segments"
category: "Memory Architecture"
priority: "P0"
version: "1.0.0"
related:
  - "01-iron-rules.md (IRON-02: Zero-Copy at Boundary)"
  - "02-build-control.md (Memory Partitioning)"
  - "03-ffi-boundary.md (WasmSlice)"
  - "04-memory-lifecycle.md (Arena Lifecycle)"
  - "../rust-architecture-guide/references/30-memory-layout.md (Memory Layout Transparency)"
  - "../rust-architecture-guide/references/31-breakwater-pattern.md (Breakwater Architecture)"
---

# Zero-Copy Resource Pool Core Implementation Specification

As the physical companion infrastructure for the "Zero-Copy Command Bus V2.0", the resource pool resolves the serialization and allocation contradiction when passing variable-length data (strings, blobs, path data) between JS and WebAssembly.

## I. Resource Pool Topology

The resource pool must occupy a continuous region in linear memory, divided into a **Static Segment** (persistent) and a **Transient Segment** (frame-ephemeral).

```rust
// wasm-kernel/src/pool/layout.rs
use std::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
pub struct ResourcePoolHeader {
    /// Transient segment write cursor, reset per frame
    pub transient_write_offset: AtomicU32,
    /// Static segment write cursor, monotonically increasing
    pub static_write_offset: AtomicU32,
    pub transient_capacity: u32,
    pub static_capacity: u32,
}

/// Resource handle: the scalar pair passed through the command bus
#[repr(C)]
pub struct ResourceHandle {
    pub offset: u32,
    pub len: u32,
}
```

### Design Principles

| Principle | Rationale |
|-----------|-----------|
| **`#[repr(C)]` mandatory** | Ensures stable binary layout across FFI; enables JS `DataView` to read offsets directly |
| **AtomicU32 cursors** | Write cursors must be visible to both JS (writer) and Wasm (resetter) without UB |
| **Contiguous linear memory** | Single `Vec<u8>` backing, split logically by capacity boundaries — no fragmentation |
| **Handle = offset + len** | Scalar-only FFI: no pointers, no `JsValue`, no `WasmSlice` complexity on the bus |

### Memory Layout

```
[ ResourcePoolHeader (16 bytes) ]
[ Static Segment (static_capacity bytes)  ]  ← persistent, monotonically growing
[ Transient Segment (transient_capacity)  ]  ← per-frame, reset on rAF start
```

**Constraint**: `transient_capacity` must be a multiple of 4 bytes ([F-07 Memory Alignment Audit](#v-hard-constraints)).

## II. JS-Side Injection Contract

JS must **never** produce temporary `Uint8Array` copies via `TextEncoder.encode()`. Encoding must execute directly inside the Wasm linear memory view.

```typescript
// web-client/src/pool/Facade.ts
export class ResourcePoolFacade {
    private view: Uint8Array;
    private encoder: TextEncoder = new TextEncoder();

    constructor(
        memory: WebAssembly.Memory,
        private ptr: number,
        private headerPtr: number
    ) {
        this.view = new Uint8Array(memory.buffer);
    }

    /**
     * Write a string directly into the transient segment and return a handle.
     * Zero-copy: TextEncoder.encodeInto writes directly into Wasm memory.
     */
    public pushTransientString(str: string): ResourceHandle {
        const header = new DataView(this.view.buffer, this.headerPtr, 16);
        const offset = header.getUint32(0, true); // transient_write_offset

        // encodeInto: zero-copy write into Wasm linear memory
        const result = this.encoder.encodeInto(
            str,
            this.view.subarray(this.ptr + offset)
        );

        const len = result.written || 0;
        header.setUint32(0, offset + len, true); // advance cursor

        return { offset, len };
    }

    /**
     * Write binary blob directly into the transient segment.
     */
    public pushTransientBlob(data: Uint8Array): ResourceHandle {
        const header = new DataView(this.view.buffer, this.headerPtr, 16);
        const offset = header.getUint32(0, true);

        this.view.set(data, this.ptr + offset);
        const len = data.length;
        header.setUint32(0, offset + len, true);

        return { offset, len };
    }
}
```

### Anti-Patterns

| Anti-Pattern | Why It Violates Zero-Copy | Correction |
|-------------|--------------------------|------------|
| `new TextEncoder().encode(str)` | Produces a temporary `Uint8Array` — one extra heap allocation + copy | Use `encodeInto` directly into `view.subarray(...)` |
| Passing `string` across FFI | Requires JSON serialization or `wasm-bindgen` string conversion — allocates in Wasm heap | Write to pool first, pass `offset/len` scalars |
| Creating new `DataView` per call | Unnecessary GC pressure | Reuse the same `DataView` for the frame lifecycle |

## III. Wasm-Side Resolution Logic

Resolution must follow V2.0 command bus **"Physical Safety"** iron rules: mandatory slice-view access with boundary interception before any dereference.

```rust
// wasm-kernel/src/pool/resolver.rs
use core::slice;
use core::str;

pub struct ResourceResolver<'a> {
    pool_data: &'a [u8],
}

impl<'a> ResourceResolver<'a> {
    /// Construct from raw pool pointer and total length.
    /// 
    /// # Safety
    /// - `ptr` must point to a valid, initialized region of at least `len` bytes
    /// - `ptr` must remain valid for the entire lifetime `'a`
    pub unsafe fn from_raw(ptr: *const u8, len: usize) -> Self {
        Self {
            pool_data: slice::from_raw_parts(ptr, len),
        }
    }

    /// Safely resolve a string view from a resource handle.
    /// 
    /// Zero-copy: returns `&'a str` — a direct view into the pool, no allocation.
    pub fn resolve_str(&self, handle: ResourceHandle) -> Result<&'a str, PoolError> {
        let offset = handle.offset as usize;
        let end = offset.saturating_add(handle.len as usize);

        // Boundary interception — the "breakwater" at the pool edge
        if end > self.pool_data.len() {
            return Err(PoolError::OutOfBounds);
        }

        let bytes = &self.pool_data[offset..end];

        // Zero-copy UTF-8 validation (the "real" strike in the core layer)
        str::from_utf8(bytes).map_err(|_| PoolError::InvalidUtf8)
    }

    /// Resolve a binary blob view from a resource handle.
    /// 
    /// Zero-copy: returns `&'a [u8]` — a direct view into the pool.
    pub fn resolve_blob(&self, handle: ResourceHandle) -> Result<&'a [u8], PoolError> {
        let offset = handle.offset as usize;
        let end = offset.saturating_add(handle.len as usize);

        if end > self.pool_data.len() {
            return Err(PoolError::OutOfBounds);
        }

        Ok(&self.pool_data[offset..end])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    OutOfBounds,
    InvalidUtf8,
}
```

### Design Rationale

| Decision | Rationale |
|----------|-----------|
| `saturating_add` | Prevents `offset + len` overflow from wrapping into valid range — a classic UB vector |
| Boundary check before slice | The "breakwater" — no illegal state reaches the core |
| `str::from_utf8` not `unsafe` | Core layer pursues determinism; UTF-8 validation is O(n) and unavoidable unless explicitly traded off |
| Return `&'a str` not `String` | Zero-copy: the view borrows from the pool, no allocation |

## IV. Frame Lifecycle Synchronization

The transient segment and command bus must share a lifecycle, ensuring **birth-death consistency**.

```
┌─────────────────────────────────────────────────────────┐
│  Frame Lifecycle (rAF Cycle)                            │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  1. rAF Start (Wasm Tick)                               │
│     ├─ transient_write_offset.store(0, Ordering::Release)│
│     └─ command_bus.reset()                              │
│                                                         │
│  2. JS Interaction Phase                                │
│     ├─ JS writes strings/blobs → pool, gets handle      │
│     └─ JS pushes handle-wrapped commands → bus          │
│                                                         │
│  3. Consumption Phase (Wasm Process)                    │
│     ├─ Wasm scans command bus, extracts handles         │
│     ├─ Wasm resolves handles → pool slice views         │
│     └─ Execute business logic on zero-copy data         │
│                                                         │
│  4. rAF End                                             │
│     └─ Transient segment implicitly invalidated          │
│        (next frame resets cursor to 0)                  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Synchronization Contract

| Phase | Actor | Action | Guarantee |
|-------|-------|--------|-----------|
| Reset | Wasm | `transient_write_offset = 0` | All transient handles from previous frame are now invalid |
| Write | JS | `pushTransientString()` → `offset/len` | Data is written, cursor advanced |
| Consume | Wasm | `resolve_str(handle)` | View is valid only within this frame |
| Invalidate | Implicit | Next frame reset | Wasm must not retain transient handles |

## V. Hard Constraints

| ID | Constraint | Rationale |
|----|-----------|-----------|
| **[F-05] No Cross-Frame Handle Retention** | Transient segment handles become invalid after `transient_write_offset` resets. Wasm must never store them in `static` collections. | Use-after-free: the offset region will be overwritten by the next frame |
| **[F-06] Mandatory UTF-8 Pre-Validation** | If business logic is extremely latency-sensitive, JS may guarantee UTF-8 legality and Wasm uses `str::from_utf8_unchecked`. Risk must be documented in ADR. | Trade-off: saves O(n) validation at the cost of potential UB on invalid input |
| **[F-07] Memory Alignment Audit** | `transient_capacity` must be a multiple of 4 bytes. Prevents alignment loss when writing `f32` payloads, which causes performance degradation on some architectures. | Unaligned access on ARM/WASM may trap or require extra instructions |

### Alignment Verification (Compile-Time)

```rust
// Ensure transient_capacity is 4-byte aligned at runtime
const fn assert_alignment(capacity: u32) {
    assert!(capacity % 4 == 0, "transient_capacity must be 4-byte aligned");
}

// Or use static_assertions crate
static_assertions::const_assert!(TRANSIENT_CAPACITY % 4 == 0);
```

## VI. Self-Check List

- [ ] JS side uses `TextEncoder.encodeInto` to write directly into Wasm memory (not `encode()`)
- [ ] Wasm side `resolve_str` executes `pool_data.len()` boundary check before any dereference
- [ ] `ResourcePoolHeader` is marked with `#[repr(C)]`
- [ ] Static segment and transient segment physical boundaries are clearly partitioned at instantiation and do not overlap
- [ ] `transient_capacity` is a multiple of 4 bytes
- [ ] Wasm does not retain transient handles across frame boundaries
- [ ] If `str::from_utf8_unchecked` is used, risk is documented in ADR
- [ ] `ResourceHandle` is `#[repr(C)]` — offset and len are `u32` scalars, not pointers
