# Control Flow: Intercepting Deep Nesting

> **Philosophy — Intercepting Boilerplate (拦截样板, Flatten Control Flow)**: If a logic can be expressed in 1 line of pattern matching, never use 5 lines of nesting.

---

## 1.1 Intercept Nesting: Use `let else`

**Rule**: When destructuring fails and requires immediate return, prohibit `if let` or `match` causing rightward code drift. Use `let else` to keep the path flat.

```rust
// ❌ Path crouching (nested)
if let Some(user) = get_user() {
    if let Ok(config) = load_config(user) {
        process(config);
    }
}

// ✅ Intercepting way (flat)
let Some(user) = get_user() else { return };
let Ok(config) = load_config(user) else { return };
process(config);
```

## 1.2 Intent Direct: Use `matches!` Macro

**Rule**: When only checking enum variants without extracting data, prohibit verbose `match`.

```rust
// ❌ Verbose
let is_valid = match state {
    State::Active | State::Pending => true,
    _ => false,
};

// ✅ Concise
let is_valid = matches!(state, State::Active | State::Pending);
```
