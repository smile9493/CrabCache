# Memory Economy: Eliminating Unnecessary Copies

> **Philosophy — Economy of Motion (经济法则, Zero-Copy Efficiency)**: Every line of code should point directly to intent. Eliminate redundant intermediate variables and implicit copies.

---

## 4.1 Borrow First: `AsRef` & Slices

**Rule**: If a function doesn't take ownership, prefer accepting `&str` or `&[u8]`, or use `impl AsRef<str>` for maximum compatibility.

```rust
// ❌ Forced allocation
fn process(s: &String) { ... }

// ✅ Economy of motion — accepts &str, String, Box<str>, etc.
fn process(s: impl AsRef<str>) {
    let s = s.as_ref();
    // ...
}
```

## 4.2 Zero-Copy View: Master `Cow`

**Rule**: When processing data that may need modification or may only need reading, use `std::borrow::Cow`.

```rust
use std::borrow::Cow;

fn normalize(input: &str) -> Cow<str> {
    if input.chars().any(|c| c.is_uppercase()) {
        Cow::Owned(input.to_lowercase())
    } else {
        Cow::Borrowed(input)  // Zero copy — no allocation
    }
}
```
