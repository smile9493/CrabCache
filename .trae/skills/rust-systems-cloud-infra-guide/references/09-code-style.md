---
title: "Code Style & Unsafe Boundaries"
description: "Capacity-aware allocation, mandatory SAFETY comments, and FFI panic catching"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["rust-architecture-guide/references/15-ffi-interop.md"]
dependents: ["10-ci-lints.md"]
---

# Skill: Code Style & Unsafe Boundaries

## 👤 Profile

* **Domain**: All Rust projects following this specification.
* **Environment**: CI with Clippy deny-level lints.
* **Philosophy**:
    * **Minimize Unsafe**: Only wrap truly needed operations; business logic goes outside.
    * **Mandatory SAFETY Comments**: Every `unsafe` block must list all preconditions.
    * **Enforce #[must_use]**: Return values must be handled.

---

## ⚔️ Core Directives

### Action 1: Explicit Capacity Awareness
* **Scenario**: Create dynamic containers.
* **Red Line**: Prohibit `Vec::new()` without capacity pre-allocation.
* **Execution**: Use `Vec::with_capacity(expected_len)`.

### Action 2: Avoid Implicit Allocation in Hot Paths
* **Scenario**: Inside loops or high-frequency call paths.
* **Red Line**: Prohibit `format!`, `.to_string()` inside loops.
* **Execution**: Use stack buffers (`arrayvec`, `itoa`) or zero-allocation interfaces.

### Action 3: Mandatory SAFETY Comments
* **Scenario**: Every `unsafe` block.
* **Red Line**: Must list all preconditions above (non-null, aligned, lifetime, no aliasing).

### Action 4: FFI Panic Catching
* **Scenario**: All `extern "C"` functions.
* **Red Line**: Must use `catch_unwind` to prevent UB.

### Action 5: Prohibit Holding Sync Locks Across `.await`
* **Scenario**: Use `std::sync::Mutex` in async code.
* **Red Line**: Strictly prohibit any sync lock crossing `.await` points.
* **CI**: Enable `clippy::await_holding_lock`.

### Action 6: Exhaustive Pattern Matching
* **Scenario**: Match internal `enum`.
* **Red Line**: Prohibit using `_` wildcard; must explicitly list all variants.

---

## 💻 Code Paradigms

### Paradigm A: Explicit Capacity

```rust
// ❌ Prohibited
let mut v = Vec::new();

// ✅ Mandatory
let mut v = Vec::with_capacity(expected_len);
```

### Paradigm B: Zero-Allocation Formatting

```rust
// ❌ Prohibited
fn process(items: &[Item]) -> String {
    let mut result = String::new();
    for item in items {
        result.push_str(&format!("{:?}", item));
    }
    result
}

// ✅ Recommended
use arrayvec::ArrayString;

fn process_fast(item: &Item) -> ArrayString<128> {
    let mut buf = ArrayString::new();
    write!(buf, "{:?}", item).ok();
    buf
}
```

### Paradigm C: SAFETY Comment

```rust
impl Buffer {
    pub fn get(&self, index: usize) -> Option<&u8> {
        if index >= self.len {
            return None;
        }
        
        // SAFETY:
        // - index < self.len verified
        // - self.ptr is valid and points to len valid elements
        // - returned lifetime bound to self
        unsafe { Some(&*self.ptr.add(index)) }
    }
}
```

### Paradigm D: FFI Panic Catching

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

#[no_mangle]
pub unsafe extern "C" fn engine_write(
    engine: *mut Engine,
    key: *const libc::c_char,
    key_len: usize,
) -> libc::c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees engine pointer is valid
        let engine = unsafe { &mut *engine };
        let key = unsafe { std::slice::from_raw_parts(key as *const u8, key_len) };
        match engine.write(key) {
            Ok(_) => 0,
            Err(_) => -1,
        }
    }));
    
    match result {
        Ok(code) => code,
        Err(_) => {
            tracing::error!("FFI panic intercepted");
            -2
        }
    }
}
```

### Paradigm E: Narrow Critical Section

```rust
struct SharedState {
    cache: Mutex<LruCache<Key, Value>>,
}

impl SharedState {
    fn get(&self, key: &Key) -> Option<Value> {
        let value = {
            let cache = self.cache.lock().expect("cache poisoned");
            cache.get(key).cloned()
        };  // Lock released here
        
        if let Some(v) = value {
            return Some(v);
        }
        
        // Expensive operation outside lock
        let computed = expensive_load(key);
        
        {
            let mut cache = self.cache.lock().expect("cache poisoned");
            cache.put(key.clone(), computed.clone());
        }
        
        Some(computed)
    }
}
```

### Paradigm F: Exhaustive Pattern Matching

```rust
enum State {
    Idle,
    Connecting,
    Connected,
    Disconnecting,
}

// ❌ Prohibited
fn handle(&mut self, state: State) {
    match state {
        State::Idle => self.start(),
        State::Connected => self.process(),
        _ => {}
    }
}

// ✅ Mandatory
fn handle(&mut self, state: State) {
    match state {
        State::Idle => self.start(),
        State::Connecting => self.wait(),
        State::Connected => self.process(),
        State::Disconnecting => self.cleanup(),
    }
}
```
