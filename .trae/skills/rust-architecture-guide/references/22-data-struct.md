# Data Structures & Initialization

## 1. Field Initialization Shorthand

**Rule**: Never repeat variable names when they match field names.

```rust
let email = "admin@test.com".to_string();
let age = 30;

// ❌ Verbose
let user = User { email: email, age: age };

// ✅ Elegant
let user = User { email, age };
```

## 2. Avoid Type Stuttering

**Rule**: Use module scope for naming. Don't repeat module name in type name.

```rust
// ❌ Stuttering
use user::UserConfig;
let config = UserConfig::new();

// ✅ Clean
use user::Config;
let config = user::Config::default();
```

## 3. Struct Update Syntax

```rust
// ✅ Override only needed fields
let config = ServerConfig {
    port: 8080,
    ..ServerConfig::default()
};
```

## 4. Tuple Structs

```rust
// ✅ Semantic isolation
struct UserId(u64);
struct OrderId(u64);
```
