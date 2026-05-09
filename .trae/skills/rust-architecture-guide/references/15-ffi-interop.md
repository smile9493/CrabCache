---
title: "FFI & Cross-Language Interop: The Defense Wall"
description: "Three-layer isolation model for safe FFI boundaries with C/C++"
category: "Advanced"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: []
dependents: ["rust-systems-cloud-infra-guide/references/03-syscall.md"]
---

# FFI & Cross-Language Interop: The Defense Wall

> **Core Philosophy — The Defense Wall**: FFI boundaries are the "no-man's land" of the system. Rust's safety guarantees become void here. The architect must construct a physical isolation wall: outside the wall is the chaotic and unsafe C ABI; inside the wall is the rigorous Rust safety model. Any attempt to penetrate this wall must pass strict contract review.

---

## 1. Boundary Architecture: Three-Layer Isolation Model

> **Intercepting Way (截拳道方式, Defense at Boundary) — Intercept complexity at the outermost layer.**

### 1.1 `sys` Layer: Raw Protocol Mirror

**Rule**: Create a standalone `*-sys` crate. Contains only raw `extern "C"` declarations, `struct` definitions, and constants.

**Automation**: Mandate using `bindgen` to auto-generate bindings. Strictly prohibit manually maintaining thousands of lines of C header file mirrors.

### 1.2 Wrapper Layer: Safe Breakwater

**Rule**: Build Safe Rust abstractions on top of the `sys` layer. Responsible for handling `unsafe` blocks, pointer validation, and lifetime mapping.

### 1.3 Business Layer: Idiomatic API

**Rule**: The final interface exposed to users should be completely unaware that the underlying implementation is C/C++. Use `Result` instead of error codes, use iterators instead of raw pointer offsets.

---

## 2. Memory & Ownership: Physical Contracts

> **Physical Transparency — Clarify who holds the "power of life and death" over memory.**

### 2.1 Opaque Pointer Protection

**Rule**: When exposing Rust objects to external languages, **mandate** passing through raw pointers. Internally use `Box::into_raw` to relinquish ownership; externally only hold the handle.

**Defense**: C code must absolutely not dereference or modify this pointer. All operations must call back into Rust-exported functions.

```rust
#[no_mangle]
pub extern "C" fn create_engine() -> *mut Engine {
    Box::into_raw(Box::new(Engine::new()))
}

#[no_mangle]
pub unsafe extern "C" fn destroy_engine(ptr: *mut Engine) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}
```

### 2.2 Deterministic Destruction

**Rule**: Any memory allocated and exported by Rust must have a dedicated `*_free` or `*_destroy` function exported.

**Red Line**: Strictly prohibit using C's `free()` to release Rust-allocated memory (and vice versa), avoiding Heap Corruption from allocator mismatch.

### 2.3 Zero-Copy View Principle

**Rule**: When processing large data across boundaries, prefer passing pointer and length (Buffer + Len), using `std::slice::from_raw_parts` to construct transient views, reducing performance degradation from memory copies.

---

## 3. Safety & Resilience: Intercepting Panic

> **Intercepting Panic — Prevent logic bombs from penetrating the boundary.**

### 3.1 Cross-Language Panic Death Ban

**Red Line Rule**: **All** exported `extern "C"` functions must be wrapped in `std::panic::catch_unwind`.

**Reason**: Rust Panic penetrating C ABI is undefined behavior (UB), causing direct process crash or memory corruption.

### 3.2 Error Code Conversion System

**Rule**: After `catch_unwind`, map the result to integer error codes (e.g., `0` for success, negative for error) or specific flags.

```rust
#[no_mangle]
pub unsafe extern "C" fn rust_api_call(ptr: *mut MyObj) -> i32 {
    let result = std::panic::catch_unwind(|| {
        // Execute specific logic
        0
    });
    result.unwrap_or(-1) // Intercept Panic, return error code
}
```

---

## 4. Data Layout: Unity of Opposites (ABI Compatibility)

> **Conform to Physical Laws — Enforce memory alignment consistency.**

### 4.1 Layout Determinism (`#[repr(C)]`)

**Rule**: All `struct` involved in cross-language transmission must be annotated with `#[repr(C)]`.

**Warning**: Prohibit using Rust-specific `Vec`, `String`, `HashMap` etc. in FFI structs (their memory layout is unstable).

### 4.2 Enum Compatibility Strategy

**Rule**: For cross-language enums, use `#[repr(i32)]` or `#[repr(u8)]`.

**Null Pointer Semantics**: For optional function pointers, leverage Rust's optimization treating `Option<extern "C" fn()>` as nullable C function pointers.

---

## 5. Build & Engineering: Proper Tools

### 5.1 Linking Strategy

**Rule**: Library-level projects should prefer static linking (Static Link), packaging C dependencies into Rust artifacts to simplify distribution complexity.

### 5.2 `cc` Helper & Cross-Compilation

**Rule**: For small amounts of C source code, compile directly in `build.rs` using the `cc` crate.

**Defense**: Configure multi-platform compilation (Linux/macOS/Windows) in CI, ensuring byte order and alignment determinism across architectures.

---

## 6. Agent FFI Checklist

1. **Are exported functions wrapped in `catch_unwind`?**
2. **Are cross-boundary structs marked with `#[repr(C)]`?**
3. **Are explicit resource release functions provided?**
4. **Is there a `// SAFETY:` comment above `unsafe` blocks proving pointer origin?**
5. **Is `std::ptr::NonNull` used instead of raw pointers to optimize null checks?**

---

**Reduction Directive**: When building cross-language interop, the Agent must be as rigorous as deploying defenses on a national border. Every byte of data flowing into or out of Rust must undergo strict C ABI baptism, ensuring that no matter how chaotic the outside world, the inside maintains type safety and physical stability.
