---
title: "Zero-Copy Command Bus V3.1"
description: "Ultimate physical protocol for zero-copy JS↔Wasm instruction pipeline: double-buffering topology, atomic synchronization, Facade write/consume dispatch cycle, and lifecycle safety contract"
category: "Architecture"
priority: "P0"
version: "3.1.0"
related:
  - "01-iron-rules.md (IRON-02: Zero-Copy at Boundary)"
  - "03-ffi-boundary.md (WasmSlice)"
  - "04-memory-lifecycle.md (Arena Lifecycle)"
  - "09-zero-copy-pool.md (Resource Pool)"
  - "../rust-architecture-guide/references/30-memory-layout.md (Memory Layout Transparency)"
  - "../rust-architecture-guide/references/31-breakwater-pattern.md (Breakwater Architecture)"
---

# Zero-Copy Command Bus Ultimate Physical Protocol V3.1

## 0. Philosophical Foundation

- **Dialectical Materialism**: Acknowledge that linear memory base address is uncontrollable, hence enforce alignment on relative offsets. Acknowledge that JS's dynamic writing and Wasm's static reading are a fundamental contradiction — physically isolate them with double buffering, unity of opposites.
- **Unity of False and Real**:
  - **False (虚)**: JS side provides ergonomically-natural `drawRect(...)` Facade methods. After calling, only scalar values are pushed into the buffer — deferred until `flush` for batch submission. This is the "false move gathering momentum" (虚招蓄力).
  - **Real (实)**: Wasm side consumes the Back Buffer at the frame boundary in one shot, dispatching directly to physical rendering through safe slice views. This is the "real strike delivering force" (实招发力).
  - **False and Real Generate Each Other**: Atomic swap (`AcqRel`) flips Front/Back — the false conceals, the real strikes, the rhythm is seamless.

## 1. Memory Physical Layout

### 1.1 Double Buffer Structure

```rust
use std::sync::atomic::AtomicU32;

/// The double-buffer management header, placed at a fixed linear memory offset.
#[repr(C)]
pub struct DoubleBuffer {
    /// Current write buffer index (0 or 1), written by JS, read by Wasm.
    pub write_index: AtomicU32,
    /// Ready flag: when JS completes writing for a frame, set to 1; Wasm zeros it after consumption.
    pub ready: AtomicU32,
    /// Capacity of each buffer (in bytes).
    pub capacity: u32,
    /// Explicit alignment reserved.
    _reserved: u32,
}

// Compile-time size verification
const _: () = assert!(std::mem::size_of::<DoubleBuffer>() == 16);

// Buffer 0 and Buffer 1 follow DoubleBuffer immediately:
//   address = base + header_size + index * capacity
```

### 1.2 Memory Layout Diagram

```
[ DoubleBuffer Header (16 bytes) ]
[ Buffer 0 (capacity bytes)      ]  ← write or read depending on index
[ Buffer 1 (capacity bytes)      ]  ← the other buffer for swap
```

## 2. Atomic Synchronization Protocol (Mandatory Memory Ordering)

### JS Write & Submit

```typescript
// In flush(), after JS completes writing to the current write_index buffer:
atomics.store(ready_offset, 1, 'release');       // make writes visible to Wasm
atomics.store(write_index_offset, nextIndex, 'release'); // switch write index
```

### Wasm Consume

```rust
// At the start of each frame tick():
let ready = self.ready.load(Ordering::Acquire);
if ready == 0 { return; }

let read_index = self.write_index.load(Ordering::Acquire) ^ 1; // consume the non-current-write buffer
let back_buffer_ptr = self.get_buffer_ptr(read_index);

// ... consume the buffer ...

// After consumption:
self.ready.store(0, Ordering::Release); // signal JS can resume writing
```

**Philosophical Mapping**: `Release` ensures "momentum is fully gathered" (蓄力已满), `Acquire` ensures "force is visible" (发力可见) — precise control of potential energy conversion.

## 3. Instruction Encoding Protocol

Following V3.0 unified variable-length header:

| Byte Offset | Field | Type | Description |
|-------------|-------|------|-------------|
| 0 | OpCode | `u8` | `0x00` NOP, `0x01` DrawRect, ... |
| 1 | PacketLen | `u8` | Total packet bytes (including header) |
| 2~3 | Padding | `u8[2]` | Align to 4 bytes |
| 4~N | Payload | - | Payload; multi-byte scalars must be 4-byte aligned |

**Constraints**:
1. Payload must absolutely never embed heap pointers or references.
2. Multi-byte scalar fields within the payload must be aligned to 4-byte boundaries.

## 4. JS-Side Write (False Move Gathering Momentum)

### 4.1 Single DataView

```typescript
export class EngineFacade {
    private view: DataView;
    private writeIndex: number;
    private bufferCapacity: number;
    private cursor: number = 0;
    private basePtr: number;
    private baseBuf0Ptr: number;
    private baseBuf1Ptr: number;
    private memory: WebAssembly.Memory;
    private wasmInstance: any;

    constructor(
        memory: WebAssembly.Memory,
        basePtr: number,
        capacity: number,
        wasmInstance: any
    ) {
        this.memory = memory;
        this.basePtr = basePtr;
        this.bufferCapacity = capacity;
        this.wasmInstance = wasmInstance;

        const buf0Ptr = basePtr + 16; // header size
        const buf1Ptr = buf0Ptr + capacity;
        this.baseBuf0Ptr = buf0Ptr;
        this.baseBuf1Ptr = buf1Ptr;

        // Initial write index is 0
        this.writeIndex = 0;
        this.view = new DataView(memory.buffer, buf0Ptr, capacity);
    }

    private ensureSpace(size: number) {
        if (this.cursor + size > this.bufferCapacity) {
            this.flush(); // buffer full, force commit and switch
        }
    }

    public drawRect(x: number, y: number, w: number, h: number): void {
        const cmdSize = 20;
        this.ensureSpace(cmdSize);
        const off = this.cursor;
        this.view.setUint8(off, 0x01);            // OpCode
        this.view.setUint8(off + 1, cmdSize);     // PacketLen
        this.view.setFloat32(off + 4, x, true);
        this.view.setFloat32(off + 8, y, true);
        this.view.setFloat32(off + 12, w, true);
        this.view.setFloat32(off + 16, h, true);
        this.cursor += cmdSize;
    }

    public flush(): void {
        if (this.cursor === 0) return;

        const nextIndex = this.writeIndex ^ 1;

        // Atomic operation: set ready flag and switch write index
        // (via exported Wasm function or atomic helpers)
        this.wasmInstance.commit_buffer(this.writeIndex, this.cursor);

        // Reset cursor and switch to the other buffer
        this.cursor = 0;
        this.writeIndex = nextIndex;

        // Update DataView to the new buffer
        const newBufPtr = (nextIndex === 0) ? this.baseBuf0Ptr : this.baseBuf1Ptr;
        this.view = new DataView(this.memory.buffer, newBufPtr, this.bufferCapacity);
    }
}
```

**Note**: `commit_buffer` is a Wasm-exported function that internally executes `ready.store(1, Release)` and `write_index.store(nextIndex, Release)` to guarantee atomicity.

### Anti-Patterns

| Anti-Pattern | Why It Violates the Protocol | Correction |
|-------------|------------------------------|------------|
| Business logic in Facade methods | JS must only push scalars, not make decisions | Facade methods are pure scalar writers only |
| `TextEncoder.encode()` for command strings | Produces temporary `Uint8Array` copy | Use `encodeInto` directly into buffer view |
| Creating new `DataView` per command | Unnecessary GC pressure | Single `DataView` per buffer, switch on `flush()` |

## 5. Wasm-Side Consume (Real Strike Delivering Force)

```rust
use core::slice;

pub fn tick(&mut self) {
    let ready = self.header.ready.load(Ordering::Acquire);
    if ready == 0 { return; }

    let read_index = self.header.write_index.load(Ordering::Acquire) ^ 1;
    let ptr = self.get_back_buffer_ptr(read_index);
    let len = self.buffer_capacity;

    // Safe slice — the only way to access buffer data
    let data = unsafe { slice::from_raw_parts(ptr, len as usize) };
    let mut cursor = 0;

    while cursor < data.len() {
        let packet_len = *data.get(cursor + 1).unwrap_or(&0) as usize;
        if packet_len < 2 || cursor + packet_len > data.len() {
            break; // illegal instruction, circuit breaker
        }

        let packet = &data[cursor..cursor + packet_len];

        match packet[0] {
            0x01 => {
                let x = f32::from_le_bytes(packet[4..8].try_into().unwrap());
                let y = f32::from_le_bytes(packet[8..12].try_into().unwrap());
                let w = f32::from_le_bytes(packet[12..16].try_into().unwrap());
                let h = f32::from_le_bytes(packet[16..20].try_into().unwrap());
                // Execute drawing (the real strike)
                self.renderer.draw_rect(x, y, w, h);
            }
            0x00 => break, // NOP
            _ => break,    // unknown opcode, stop
        }

        cursor += packet_len;
    }

    // Consumption complete, release ready flag
    self.header.ready.store(0, Ordering::Release);
}
```

### Design Rationale

| Decision | Rationale |
|----------|-----------|
| `slice::from_raw_parts` only | The "single slice safety" principle — no raw pointer arithmetic in the consume loop |
| `data.get(cursor + 1)` | Boundary interception before dereference |
| `packet_len < 2` guard | Minimum valid packet size — malformed packets trigger immediate circuit breaker |
| `break` on unknown opcode | Fail-fast — no attempt to recover from corrupt data |
| `ready.store(0, Release)` | Signal JS that the buffer is free for next frame |

## 6. Lifecycle Safety Contract

All data passed through zero-copy views must obey:

| Contract | Rule |
|----------|------|
| **Synchronous Consumption** | WasmSlice obtained by JS must only be used within the current synchronous task — never stored in async callbacks or global state |
| **Frame-Scoped Validity** | Contents in the double buffer are considered invalid after Wasm calls `ready.store(0)`. JS must not read them again |
| **No Cross-Boundary Leakage** | Instruction packets must never contain pointers to Wasm heap or JS heap |
| **Single Buffer Continuity** | Instruction packets must be continuous within the same buffer — never split across buffers ([F-11](#7-hard-constraints)) |

## 7. Hard Constraints

| ID | Constraint | Rationale |
|----|-----------|-----------|
| **[F-08] No Business Logic in Facade** | JS Facade methods must not execute any business logic decisions | Facade is a pure scalar writer — decisions belong in Wasm core |
| **[F-09] No Allocation in Consume Loop** | Heap allocation or formatting is absolutely prohibited during the consume loop | Allocation during rendering causes frame stutter and GC pressure |
| **[F-10] Explicit Ordering Mandatory** | All atomic operations must be explicitly annotated with `Ordering` and documented | Implicit `SeqCst` or missing ordering is UB in cross-language contexts |
| **[F-11] No Cross-Buffer Packet Splitting** | Instruction packets must be continuous within the same buffer — never span buffer boundaries | A packet split across buffers would be consumed as two incomplete fragments |

## 8. Infrastructure Selection

| Scale | Allocator | Rationale |
|-------|-----------|-----------|
| **Small/Micro** | `mini-alloc` (page zero-fill) | Binary size < 1KB, suitable for minimal Wasm modules |
| **Medium/Large** | `talc` (supports `free` semantics) | Proper deallocation needed for complex scenes with dynamic resource lifecycle |

## 9. Self-Check List

- [ ] Double buffer header struct is `#[repr(C)]` and compile-time size verified as 16 bytes
- [ ] JS side writes exclusively via `DataView`
- [ ] Wasm consumption strictly through slices — no raw pointer arithmetic
- [ ] All atomic operations explicitly annotated with `Ordering` and commented
- [ ] `drawRect` and similar false-move methods contain no business logic branches
- [ ] `flush()` performs atomic double-buffer switch and sets ready flag
- [ ] Ready flag is reset after consumption completes
- [ ] Buffer-full handling logic is configured (flush or reject)
