# Metaprogramming & Macro Magic: Intercepting Boilerplate

> **Core Philosophy — Intercepting Boilerplate (拦截样板, Compile-time Code Generation)**: Macros are not for creating "magic," but for intercepting repetitive, mechanical, and error-prone boilerplate code at compile time. The highest-level metaprogramming should be **transparent**: the code it generates should be as efficient and debuggable as hand-written code.

---

## 0. Positioning & Priority Alignment

### Priority Framework

- **Primary: P1 (Maintainability)**: Eliminate boilerplate, provide type-safe abstractions.
- **Constrained by: P2 (Compile Efficiency)**: Macro-expanded code increases compilation time; overuse degrades build speed.
- **P0 Red Line**: Macros must **never** implicitly generate unchecked `unsafe` blocks.

### Execution Mode Integration

| Mode | Enforcement | Requirements |
|------|-------------|--------------|
| `rapid` | **Relaxed** | High-frequency declarative macros allowed; proc-macros still require separate crate, but can skip `cargo-bloat` checks |
| `standard` | **Full rules** | All rules active; proc-macros require `cargo-expand` audit + `trybuild` tests |
| `strict` | **Enhanced** | Additional compile-time monitoring (`cargo-timing`); `clippy` checks on macro-expanded code |

### Design Philosophy

> **"If it can be solved with generics, traits, or functions, never use macros."**

Macros generate code that is unfriendly to IDEs and increases cognitive load for downstream developers.

**Goal**: Provide type-safe abstractions, not magical side effects.

**Terminology**: Hygiene, TokenStream, derive macros, attribute macros, etc. defined in [`glossary.md`](05-glossary.md).

---

## 1. Macro Selection Decision Tree

Before deciding to use macros, check in order:

```
1. Is it simple code substitution or repetition?
   ├─ Yes → Declarative Macro (macro_rules!)
   └─ No → Continue

2. Need to parse complex syntax or implement custom derive attributes?
   ├─ Yes → Procedural Macro (Proc-macro)
   └─ No → Continue

3. For P3 extreme performance?
   ├─ Yes → Evaluate const fn, generic specialization, or manual implementation first
   └─ No → Use functions/generics (DO NOT use macros)
```

**Key Principle**: Macros are the **last resort**, not the first choice.

---

## 2. Declarative Macros (`macro_rules!`)

### 2.1 Scope Limitation

**MUST**:
- Prohibit `#[macro_export]` except for global utilities (e.g., `vec!`-level)
- Prefer defining macros in private modules
- Export via `pub(crate)` to avoid polluting downstream namespaces

```rust
// ❌ Bad: Pollutes global namespace
#[macro_export]
macro_rules! internal_helper { ... }

// ✅ Good: Scoped export
pub(crate) use internal_macros::helper;
```

### 2.2 Readability Red Lines

**Prohibited**:
- Nested repetition patterns exceeding 3 levels
- Overly complex TT munchers (upgrade to proc-macro instead)

**SHOULD**:
- Provide clear syntax indicators for parameters (`$name:ident`, `$expr:expr`)
- Use single repetition pattern when possible

**Good Example**:
```rust
// ✅ Clear, single repetition pattern
macro_rules! vec_of_strings {
    ($($x:expr),* $(,)?) => {
        vec![$($x.to_string()),*]
    };
}

let v = vec_of_strings!["foo", "bar", "baz"];
```

**Bad Example**:
```rust
// ❌ Nested repetition > 3 levels — hard to debug
macro_rules! complex_parser {
    ($($(($($tt:tt)*),)*),*) => { ... };
}
```

### 2.3 Hygiene & Explicit Contracts

**Rule**: `macro_rules!` must maintain hygiene. **Prohibit** implicitly capturing external scope variables. All dependent identifiers must be explicitly passed as parameters.

**Purpose**: Ensure logical consistency when macros are called across different modules, avoiding naming pollution.

```rust
// ❌ Implicit capture — fragile
macro_rules! process {
    () => { $db_conn.execute(...) };
}

// ✅ Explicit contract — robust
macro_rules! process {
    ($conn:ident) => { $conn.execute(...) };
}
```

### 2.4 Complexity Control

**Rule**: Prohibit writing recursive macros (TT Munchers) with more than 3 levels of nested parsing logic.

**Judgment**: If a macro's logic starts involving complex Token transformations, it **must** be upgraded to a procedural macro.

---

## 3. Procedural Macros (Procedural Macros)

### 3.1 Compile-Time Cost Management

**MUST**:
- Procedural macros must be isolated in separate crates with `proc-macro = true`
- Keep proc-macro crates lightweight and focused

**SHOULD**:
- Enable `syn` features on-demand; disable `full` unless parsing complete Rust syntax is necessary
- Use `quote` for token generation (provides hygiene benefits)

```toml
# ✅ Good: Minimal syn dependencies
[dependencies]
syn = { version = "2.0", features = ["derive", "parsing"] }
quote = "1.0"
proc-macro2 = "1.0"

# ❌ Bad: Enables everything (slow compilation)
[dependencies]
syn = { version = "2.0", features = ["full"] }
```

**SHOULD** (`strict` mode):
- Regularly use `cargo-timing` to monitor compilation time
- Use `cargo-bloat` to check binary size inflation

### 3.2 Error Reporting & Span Accuracy

**MUST**:
- Error messages must point to the exact user code location that triggered the error (accurate `Span`)
- **Absolutely prohibit** using `panic!()` inside procedural macros
- Must emit proper `proc_macro::Diagnostic`-level errors

**Modern Toolchain**: Prefer `proc-macro-error2` crate for unified error reporting across compiler versions:

```toml
[dependencies]
proc-macro-error2 = "2"
```

```rust
use proc_macro_error2::{abort, emit_error, proc_macro_error};

#[proc_macro_error]
#[proc_macro_derive(MyTrait, attributes(my_attr))]
pub fn derive_my_trait(input: TokenStream) -> TokenStream {
    // Errors automatically attach to correct spans via syn
    let input = parse_macro_input!(input as DeriveInput);

    if input.ident.to_string().starts_with('_') {
        abort!(input.ident, "Type name must not start with underscore");
    }
    // ...
}
```

**For attribute-heavy derive macros**, use `darling` for ergonomic attribute parsing:

```toml
[dependencies]
darling = "0.20"
```

```rust
use darling::FromDeriveInput;

#[derive(FromDeriveInput)]
#[darling(attributes(my_crate))]
struct MyDeriveOpts {
    // Required field — error if missing
    feature: String,

    // Optional with default
    #[darling(default)]
    optional_flag: bool,

    // "Did you mean" suggestions for typos — automatic!
    // darling::Error::write_errors provides precise error locations
}
```

**Correct Example**:
```rust
// ✅ Correct: Error precisely points to violating field
use syn::{spanned::Spanned, Error};

return Err(Error::new_spanned(
    field_ident,
    "This field type does not support this attribute"
).into_compile_error());

// Or using quote_spanned!
quote_spanned! { field.span() =>
    compile_error!("This field type does not support this attribute");
}
```

**Wrong Example**:
```rust
// ❌ Wrong: Panic loses span information
if !supported_type {
    panic!("Unsupported type!");
}
```

### 3.3 Derive Macro Priority

**Rule**: When building plugin systems or serialization frameworks, prioritize in this order:

1. **`#[derive(Trait)]`** (most IDE-friendly, appends code only)
2. **Attribute macros `#[attr]`** (modifies structure)
3. **Function-like macros `macro!()`** (least type inference support)

**Reason**: Derive macros only append code without modifying the original structure, making them most IDE-friendly for type inference — conforming to the principle of "aligning with the physical substrate."

### 3.4 Unit Testing

**MUST**:
- Use `trybuild` to write compile-fail tests
- Ensure users get clear error messages when misusing macros

**SHOULD**:
- Provide expanded example code as part of documentation

```rust
#[test]
fn ui_tests() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs"); // Verify error messages are clear
    t.pass("tests/ui/valid/*.rs");   // Verify valid code compiles
}
```

---

## 4. Hygiene & Debugging

### 4.1 Principle of Least Astonishment

**Prohibited**:
- Implicitly introducing undeclared external dependencies in macro-generated code
- Generating code that relies on specific `use` statements in caller scope

**MUST**:
- Use absolute paths for all references (e.g., `::std::vec::Vec`)
- Avoid missing `use` statements causing compilation errors

```rust
// ❌ Bad: Relies on caller having `use std::vec::Vec;`
quote! {
    Vec::new()
}

// ✅ Good: Absolute path, always works
quote! {
    ::std::vec::Vec::new()
}
```

### 4.2 Expansion Audit

**MUST** (`standard` mode and above):
- For complex macros, use `cargo expand` to inspect expanded code
- Ensure no redundancy or potential risks

```bash
cargo expand --lib  # Review all macro expansions in the crate
cargo expand my_macro_test  # Review specific test expansion
```

**SHOULD** (`strict` mode):
- Submit `cargo-expand` output to documentation or review records
- Check for: redundant clones, unnecessary allocations, missing optimizations

**Self-check points**:
- Does the expanded code contain redundant `clone()` calls?
- Are there unnecessary heap allocations?
- Is the generated code as efficient as hand-written code?

---

## 5. Safety Boundaries

### 5.1 P0 Red Line: Unsafe Generation

**MUST**:
- Macros must **never** implicitly generate `unsafe` blocks
- If unavoidable, must force users to explicitly confirm via parameters
- Generated code must include complete `// SAFETY:` comments

**Example**:
```rust
// ❌ Forbidden: Implicit unsafe generation
macro_rules! unsafe_op {
    () => {
        unsafe {
            // Hidden danger!
        }
    };
}

// ✅ Required: Explicit user confirmation
macro_rules! unsafe_op {
    (unsafe) => {
        // SAFETY: User explicitly acknowledged unsafe semantics
        // [Detailed safety invariant explanation]
        unsafe {
            // ...
        }
    };
}
```

### 5.2 Review Checklist

**MUST**: Any macro containing `unsafe` generation must be listed separately in code reviews.

**Review points**:
- [ ] Is the `unsafe` truly necessary?
- [ ] Are safety invariants documented with `// SAFETY:` comments?
- [ ] Does the user explicitly opt-in to unsafe behavior?
- [ ] Has the expanded code been audited with `cargo expand`?

---

## 6. Toolchain & Validation

| Tool | Purpose | Mode Requirement |
|------|---------|------------------|
| `cargo expand` | View macro-expanded code | `standard`, `strict` |
| `trybuild` | Write compile-fail tests | `standard`, `strict` |
| `cargo bloat` | Check binary size inflation | `strict` |
| `cargo timing` | Monitor compilation time | `strict` (regular) |
| `clippy` | Lint macro-expanded code | `strict` |

### Usage Examples

```bash
# 1. Expand macros for review
cargo expand --lib > expanded.rs

# 2. Run compile-fail tests
cargo test --test ui_tests

# 3. Check binary size (strict mode)
cargo bloat --release

# 4. Monitor compile time (strict mode)
cargo +nightly build -Z timings
```

---

## 7. Integration with Overall Architecture

### 7.1 Design Philosophy Alignment

| Principle | Metaprogramming Application |
|-----------|----------------------------|
| **Economy of Motion** | Eliminate boilerplate, not create magic |
| **Physical Alignment** | Generated code = hand-written efficiency |
| **Unity of Opposites** | Magic must be auditable (expand, trybuild) |

### 7.2 Priority Constraints

**P1 (Maintainability)** is the primary beneficiary:
- Eliminate repetitive boilerplate
- Provide type-safe abstractions
- Reduce human error in mechanical code

**P2 (Compile Time)** is the constraint:
- Proc-macro crates add compilation overhead
- Complex `syn` parsing slows builds
- Must monitor with `cargo-timing`

**P0 (Safety)** is the red line:
- No implicit `unsafe` generation
- Explicit user opt-in for unsafe operations
- Full `// SAFETY:` comments required

### 7.3 Terminology Links

All specialized terms defined in [`glossary.md`](05-glossary.md):
- Hygiene
- TokenStream
- TT Muncher
- Span
- Derive Macro
- Attribute Macro
- Function-like Macro

### 7.4 Related Documents

- [`00-mode-guide.md`](00-mode-guide.md) — Execution modes (rapid/standard/strict)
- [`01-priority-pyramid.md`](01-priority-pyramid.md) — P0-P3 priority framework
- [`05-glossary.md`](05-glossary.md) — Metaprogramming terminology
- [`17-toolchain.md`](17-toolchain.md) — Cargo tools and CI integration
- [`26-advanced-testing.md`](26-advanced-testing.md) — UI testing with trybuild

---

## 8. Agent Metaprogramming Checklist

Before generating any macro code:

### Macro Selection
- [ ] Can this be replaced with `impl Trait` generics? (If yes, abandon macro)
- [ ] Does it follow the decision tree (Section 1)?
- [ ] Is it for boilerplate elimination (not showing off)?

### Declarative Macros
- [ ] Nested repetition ≤ 3 levels?
- [ ] Explicit parameter contracts (no implicit capture)?
- [ ] Avoided `#[macro_export]` unless global utility?
- [ ] Uses absolute paths (`::std::...`)?

### Procedural Macros
- [ ] Isolated in separate crate with `proc-macro = true`?
- [ ] Minimal `syn` features enabled?
- [ ] Error handling uses `syn::Error` with correct `Span`?
- [ ] No `panic!()` calls?
- [ ] Has `trybuild` compile-fail tests?

### Safety
- [ ] No implicit `unsafe` generation?
- [ ] If unsafe, user explicitly opts in?
- [ ] Complete `// SAFETY:` comments present?
- [ ] Listed separately in code review?

### Validation
- [ ] Passed `cargo-expand` audit? (standard/strict)
- [ ] Passed `clippy` checks on expanded code? (strict)
- [ ] Compile time monitored? (strict)
- [ ] Binary size checked? (strict)

---

## 9. Trade-offs & Decision Framework

### When to Use Macros

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| Repetitive boilerplate (e.g., trait impls for multiple types) | **Use declarative macro** | P1 > P2: Eliminates human error |
| Custom derive for serialization/framework | **Use derive macro** | P1 > P2: Type-safe abstraction |
| DSL for configuration or testing | **Consider proc-macro** | P1 > P2: Improves ergonomics |
| Simple code substitution | **Use declarative macro** | P1 > P2: Reduces repetition |
| P3 performance optimization | **Evaluate const fn first** | P2 constraint: Macros add compile time |
| Complex syntax transformation | **Use proc-macro** | Necessity: Beyond declarative capabilities |

### When NOT to Use Macros

| Scenario | Decision | Rationale |
|----------|----------|-----------|
| Can be solved with generics/functions | **Reject** | P1 > P2: IDE-friendly code |
| Showing off "clever" syntax | **Reject** | P1: Increases cognitive load |
| One-off code generation | **Reject** | P2: Not worth compile time cost |
| Implicit unsafe generation | **Absolutely Forbidden** | P0: Safety red line |

### Progressive Complexity Path

```
Simple repetition
    ↓
Declarative macro (macro_rules!)
    ↓
Complex syntax / custom derive
    ↓
Procedural macro (separate crate)
    ↓
Strict mode validation (cargo-expand, trybuild, timing)
```

---

## 10. Related

### Within rust-architecture-guide
- [`00-mode-guide.md`](00-mode-guide.md) — Execution modes governing macro enforcement
- [`01-priority-pyramid.md`](01-priority-pyramid.md) — Priority framework (P0-P3)
- [`05-glossary.md`](05-glossary.md) — Metaprogramming terminology
- [`17-toolchain.md`](17-toolchain.md) — Cargo tools (expand, bloat, timing)
- [`26-advanced-testing.md`](26-advanced-testing.md) — UI testing with trybuild

### External Resources
- [The Little Book of Rust Macros](https://veykril.github.io/tlborm/) — Comprehensive macro guide
- [proc-macro-workshop](https://github.com/dtolnay/proc-macro-workshop) — Learn by building proc-macros
- [syn crate documentation](https://docs.rs/syn) — Parsing Rust syntax

---

**Version History**: 
- v2.0 — Added mode layering, decision tree, review checklist, toolchain integration, terminology links, and safety boundaries
- v1.0 — Initial metaprogramming specification
