# Error Handling Combinators

## 1. Unwrap-or-Else for Lazy Evaluation

**Rule**: When the default value is expensive to compute, use lazy evaluation.

```rust
// ❌ Eager: formats and allocates string every time
let name = opt_name.unwrap_or(format!("user_{}", generate_id()));

// ✅ Lazy: closure only executes if opt_name is None
let name = opt_name.unwrap_or_else(|| format!("user_{}", generate_id()));
```

## 2. Map-Err and And-Then

**Rule**: Don't use `match` to unwrap and rewrap Results. Chain combinators instead.

```rust
// ❌ Verbose match decomposition
let result = match parse_input(input) {
    Ok(val) => Ok(transform(val)),
    Err(e) => Err(ApiError::from(e)),
};

// ✅ Chain combinators
let result = parse_input(input)
    .map(transform)
    .map_err(ApiError::from);
```
