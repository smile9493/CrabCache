# Data Processing: Flowing Force

> **Philosophy — Economy of Motion (经济法则, Zero-Copy Efficiency)**: Every line of code should point directly to intent. Eliminate redundant intermediate variables and implicit copies.

---

## 2.1 Declarative Iteration: Reject Manual `for` Loops

**Rule**: For collection transformation, filtering, and aggregation, enforce iterator chaining (Iterator Chaining). This is not only more concise, but leverages `fold` and `map` inlining optimization.

```rust
// ❌ Manual hauling (imperative)
let mut result = Vec::new();
for x in data {
    if x > 10 {
        result.push(x * 2);
    }
}

// ✅ Flowing force (declarative)
let result: Vec<_> = data.into_iter()
    .filter(|&x| x > 10)
    .map(|x| x * 2)
    .collect();
```

## 2.2 Combinator Art: Use `filter_map`

**Rule**: When a `map` operation produces `Option` and `None` values need filtering, merge into `filter_map` — reduce intermediate layers.

```rust
// ❌ Two-pass processing
data.iter().map(transform).filter(|x| x.is_some()).map(|x| x.unwrap())

// ✅ Instant intercept
data.iter().filter_map(transform)
```
