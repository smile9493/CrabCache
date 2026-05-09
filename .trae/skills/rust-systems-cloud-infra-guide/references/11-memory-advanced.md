# Advanced Memory & Allocators (V4.1.0)

> **📚 Prerequisites**: This document assumes understanding of basic Arena allocation from [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md) §2 (Arena Architecture) and memory optimization from [`25-performance-tuning.md`](../../rust-architecture-guide/references/25-performance-tuning.md) §2 (Arena Allocation).
> 
> **🔺 Deepening Direction**: Applying Arena patterns to database kernels, HFT systems with NUMA-aware placement, PMEM mapping, and custom Allocator API for extreme performance scenarios.
> 
> **📋 Document Profile**:
> - **Domain**: Database kernels, extreme-low-latency gateways, high-frequency trading systems.
> - **Environment**: Long-running nodes (Uptime > 1 year), 10GbE+ network, multi-NUMA architecture.
> - **Mode**: `strict` (mandatory for long-running systems)
> - **Prerequisites**: [`09-data-architecture.md`](../../rust-architecture-guide/references/09-data-architecture.md), [`25-performance-tuning.md`](../../rust-architecture-guide/references/25-performance-tuning.md)

## Philosophy

* **Materialist Dialectics (唯物辩证法, Hardware Sympathy)**: Accept memory fragmentation "entropy increase" as objective necessity; hardware heterogeneity determines upper-layer software behavior.
* **Jeet Kune Do (截拳道, Compile-time Defense First)**: "One strike, instant destruction" inch punch (Arena); flow like water to hardware channels (Allocator API).

---

## ⚔️ Core Directives

### Action 1: Arena / Bump Allocator - Lifecycle "Inch Punch" Law
* **Scenario**: Per-Request / Per-Transaction AST trees, execution plan nodes, temporary state machine variables.
* **Red Line**: **Absolutely prohibit** scattered allocation on default global heap in hot request paths.
* **Execution**:
    1. Mandate Arena (`bumpalo`) to take over all allocations within request lifecycle.
    2. At request end, Arena batch release (O(1)), no lingering.
    3. **Safety Red Line**: Prohibit escaping Arena-allocated pointers to external structures (prevent Use-After-Free).

### Action 2: Physical Addressing (Allocator API) - "Flow Like Water" to Hardware
* **Scenario**: Build database low-level B-Tree, hash table, MemTable.
* **Red Line**: Core structures **must** decouple default allocator, allow generic injection.
* **Execution**:
    1. **NUMA-Aware**: Allocate data on local NUMA node memory, eliminate cross-node latency.
    2. **PMEM Mapping**: Allocate core index on persistent memory.
    3. **Degradation Strategy**: Use `allocator_api2` (stable) instead of nightly in production.

### Action 3: Slab / Pre-allocation Bitmap - Unity of Macro and Micro
* **Scenario**: Low-level C FFI boundary or I/O buffer pools.
* **Red Line**: **Prohibit** throwing high-frequency tiny allocations directly to OS.
* **Execution**:
    1. Use `mmap` to request large contiguous block (e.g., 1GB) in one shot.
    2. Combine with `mlock` to lock physical pages (prevent swap).
    3. Use `crossbeam_queue::ArrayQueue` for O(1) lock-free allocation.

### Action 4: Memory Exhaustion Backpressure - Gracefully Face Physical Depletion
* **Scenario**: Custom allocator reaches capacity limit.
* **Red Line**: **Absolutely prohibit** direct `panic!` or undefined behavior.
* **Execution**:
    1. Return `Result<T, AllocError>`.
    2. Upper layer: reject requests (503), evict cold data (LRU), fallback to global heap.
    3. Last resort: Process initiates restart via panic.

---

## 💻 Code Paradigms

### Paradigm A: Arena Request Lifecycle Enclosure

```rust
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

/// Process single transaction, all temporary memory bound to arena
pub fn process_transaction(payload: &[u8]) -> Result<Response, ExecutionError> {
    // 1. Establish boundary: Arena born
    let arena = Bump::new(); 
    
    // 2. Internal flow: strictly limit allocation within arena
    let mut ast_nodes = BumpVec::with_capacity_in(1024, &arena);
    let parsed_node = parse_to_ast(&arena, payload)?; 
    ast_nodes.push(parsed_node);
    
    // 3. Extract result: convert final result to global heap allocation
    let response = execute_ast(&ast_nodes).into_owned();
    
    // 4. Inch punch release: thousands of AST nodes instantly reclaimed (O(1))
    Ok(response)
}
```

### Paradigm B: High-Throughput Lock-Free Slab Allocator

```rust
use crossbeam_queue::ArrayQueue;
use std::ptr::NonNull;
use std::alloc::{alloc, Layout};

/// Pre-allocated lock-free fixed-block memory pool
pub struct ZeroOverheadSlab {
    memory: NonNull<u8>,
    slot_size: usize,
    free_list: ArrayQueue<usize>, 
}

impl ZeroOverheadSlab {
    pub fn new(total_size: usize, slot_size: usize) -> Self {
        let slots = total_size / slot_size;
        let layout = Layout::from_size_align(total_size, slot_size).unwrap();
        
        // Materialist foundation: request large physical memory from system in one shot
        let ptr = unsafe { alloc(layout) };
        let memory = NonNull::new(ptr).expect("FATAL: Out of memory during init");
        
        let free_list = ArrayQueue::new(slots);
        for i in 0..slots {
            let _ = free_list.push(i);
        }
        
        Self { memory, slot_size, free_list }
    }

    /// O(1) lock-free instant allocation
    pub fn alloc(&self) -> Result<NonNull<u8>, AllocError> {
        let idx = self.free_list.pop().ok_or(AllocError::Exhausted)?;
        let offset = idx * self.slot_size;
        // SAFETY: offset strictly within pre-allocated range
        Ok(unsafe { NonNull::new_unchecked(self.memory.as_ptr().add(offset)) })
    }

    /// O(1) lock-free reclamation
    pub fn dealloc(&self, ptr: NonNull<u8>) {
        let offset = unsafe { ptr.as_ptr().offset_from(self.memory.as_ptr()) } as usize;
        let idx = offset / self.slot_size;
        let _ = self.free_list.push(idx); 
    }
}

#[derive(Debug)]
pub enum AllocError { Exhausted }
```

### Paradigm C: Physical Addressing Generic Allocator

```rust
use allocator_api2::alloc::{Allocator, Global};

/// Low-level data structure decoupling memory allocator
pub struct CustomBTree<K, V, A: Allocator = Global> {
    root: Option<NodePtr<K, V>>,
    alloc: A, // Hold concrete physical allocator instance
}

impl<K, V, A: Allocator> CustomBTree<K, V, A> {
    pub fn new_in(alloc: A) -> Self {
        Self { root: None, alloc }
    }

    fn allocate_node(&self) -> Result<NodePtr<K, V>, AllocError> {
        let layout = Layout::new::<BTreeNode<K, V>>();
        let ptr = self.alloc.allocate(layout)?;
        Ok(NodePtr::new(ptr))
    }
}
```

### Paradigm D: Memory Exhaustion Backpressure Handling

```rust
fn alloc_with_backpressure<T>(arena: &Bump) -> Result<T, AllocError> {
    match arena.try_alloc_layout(Layout::new::<T>()) {
        Ok(ptr) => Ok(unsafe { ptr.cast::<T>().read() }),
        Err(_) => {
            tracing::warn!("Arena exhausted, triggering backpressure");
            // Strategy choice:
            // 1. Return error, upper layer reject new requests (503)
            // 2. Try LRU eviction
            // 3. Fallback to global heap
            Err(AllocError)
        }
    }
}
```

---

## 📊 Architecture Benefits

| Technique | Benefit |
|-----------|---------|
| Arena | O(1) batch reclamation, eliminates fragmentation, optimal spatial locality |
| Allocator API | NUMA-aware, PMEM support, hardware conformance |
| Slab | O(1) lock-free alloc/dealloc, physical page locking |
| Backpressure | Graceful degradation, avoid OOM crash |

---

*Version V4.1.0 | Must coordinate with V3.0.0 backpressure mechanism.*
