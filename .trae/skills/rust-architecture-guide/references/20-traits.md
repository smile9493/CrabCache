# Traits & Conversion: Aligning with Physical Contracts

> **Philosophy — Hardware Sympathy (硬件同理心, Hardware-Aligned Optimization)**: Leverage iterators and zero-copy types, align with the compiler's inline optimization.

---

## 3.1 Auto-Derivation: Implement `From`, Not `Into`

**Rule**: When converting types, only implement `From<T>`. Rust automatically leverages the "unity of opposites" principle to provide the `.into()` implementation.

```rust
// ✅ Core implementation
impl From<DatabaseError> for ApiError {
    fn from(err: DatabaseError) -> Self { Self::Internal(err.to_string()) }
}
// Caller: let e: ApiError = db_err.into();
```

## 3.2 Initialization Shorthand: Field Init & `Default`

**Rule**: When variable name matches field name, use shorthand; provide reasonable `Default` to enable struct update syntax.

```rust
// ✅ Elegant construction
let config = ServerConfig {
    port,  // shorthand
    ..ServerConfig::default()  // override only necessary fields
};
```
