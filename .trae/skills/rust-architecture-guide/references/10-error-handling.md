---
title: "Error Handling Strategy"
description: "Library vs Application error handling with thiserror and anyhow, plus miette, error-stack, and wrapping patterns"
category: "Patterns"
priority: "P0-P1"
applies_to: ["rapid", "standard", "strict"]
prerequisites: []
dependents: []
aligned_with: ["Rust API Guidelines C-GOOD-ERR", "Rust Error Guidelines (azdanov)", "Safety-Critical Rust Coding Guidelines"]
---

# Error Handling Strategy

## Library vs Application Layering

### Library / Public Crate

**Must**: Define named, structured error enums. Wrap third-party errors — never expose them directly in your public API.

```rust
// ✅ Required: Structured error enum with #[non_exhaustive]
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Query failed: {0}")]
    Query(String),
    #[error("Serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ❌ Forbidden: Exposing anyhow::Error in public API
pub fn query() -> anyhow::Result<Row> { } // Don't do this!

// ❌ Forbidden: Exposing third-party error types directly
pub fn parse() -> Result<Row, serde_json::Error> { } // Wraps into your error type instead
```

> **Intercepting Third-Party Errors (截击第三方错误)**: Third-party error types are external chaos entering your system. The wrapper layer must be the "interception point" where external errors are translated into your domain's structured error types. This preserves API stability when upstream crates change their error types.

### Application / Binary

**Recommended**: Use `anyhow` for error aggregation.

```rust
use anyhow::{Context, Result};

async fn run() -> Result<()> {
    db.query().await.context("Failed to query database")?;
    Ok(())
}
```

## Panic Control

### Business Logic: Zero Panic

```rust
// ✅ Business logic: Return Option/Result
fn get_first(items: &[T]) -> Option<&T> {
    items.first()
}

// ✅ Proven safe precondition: Use expect with SAFETY comment
fn process_buffer(buf: &[u8]) {
    // SAFETY: Length verified on previous line
    let first = buf[0];
}
```

### Test Code: Allow unwrap

```rust
#[cfg(test)]
fn test_example() {
    let result = compute().unwrap(); // Allowed in tests
}
```

## Lazy Context Attachment

**Rule**: When context construction is expensive (string formatting, etc.), use lazy evaluation.

**Why**: Avoids unnecessary formatting when error is already present and prevents log redundancy from stacked contexts.

```rust
// ✅ Lazy evaluation for expensive context (must use lazy evaluation)
result.with_context(|| format!("Processing item {}", item.id))?;

// ❌ Avoid: Eager evaluation always formats (avoid eager evaluation)
result.with_context(format!("Processing item {}", item.id))?;
```

**Note**: Use `with_context` (lazy) instead of `context` (eager) from anyhow when formatting is involved.

## Error Recovery Patterns

### Retry with Backoff

For transient failures (network, rate limits), implement retry with exponential backoff:

```rust
use tokio::time::{sleep, Duration};

async fn retry_with_backoff<F, T>(mut f: F, max_retries: u32) -> anyhow::Result<T>
where
    F: FnMut() -> anyhow::Result<T>,
{
    let mut attempt = 0;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) if attempt < max_retries => {
                let delay = Duration::from_millis(100 * 2u64.pow(attempt));
                tracing::warn!(attempt, error = %e, "retrying after {:?}", delay);
                sleep(delay).await;
                attempt += 1;
            }
            Err(e) => return Err(e.context(format!("failed after {} retries", attempt))),
        }
    }
}
```

### Circuit Breaker

For external service calls, use a circuit breaker to prevent cascading failures:

```rust
// Use `tower` middleware or a dedicated crate like `breakpad`
// Rule: Never retry indefinitely — always have a circuit breaker or timeout
```

**Rules**:
- **Always set a timeout** — never let an operation hang indefinitely
- **Retry only idempotent operations** — POST with side effects is not safe to retry
- **Log every retry** — silent retries hide systemic problems

## Error Categorization

Classify errors to enable different handling strategies:

```rust
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum AppError {
    // Transient: retry may succeed
    #[error("network timeout: {0}")]
    Transient(String),

    // Permanent: retry will not help
    #[error("invalid input: {0}")]
    Permanent(String),

    // Fatal: system cannot continue
    #[error("database corruption: {0}")]
    Fatal(String),
}

impl AppError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, AppError::Transient(_))
    }
}
```

## Modern Error Crate Ecosystem

### Error Reporters

| Crate | Strengths | When to Use |
|-------|-----------|-------------|
| `anyhow` | Simple, std-compatible, ubiquitous | Default application-level |
| `eyre` + `color-eyre` | Rich spantrace, automatic backtrace | Debug-heavy applications |
| `miette` | Beautiful diagnostic output with source code snippets | CLI tools, developer-facing errors |
| `error-stack` | Attach key-value context, thread-safe reports | Structured observability, multi-layer context |

```rust
// miette: diagnostic-grade errors with source annotations
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
#[error("Parse error at {position}")]
#[diagnostic(help("Check the syntax at the indicated position"))]
pub struct ParseError {
    #[source_code]
    src: String,
    #[label("here")]
    position: SourceSpan,
}

// error-stack: structured context attachment
use error_stack::{Report, ResultExt};

fn process() -> error_stack::Result<(), AppError> {
    let config = read_config()
        .attach_printable_lazy(|| format!("while reading config for env={}", std::env::var("ENV").unwrap_or_default()))?;
    Ok(())
}
```

### Selection Decision Tree

```
Is this a library (public API)?
├─ Yes → thiserror + #[non_exhaustive] error enum
└─ No (application/binary)
   ├─ CLI tool / user-facing diagnostics?
   │  └─ Yes → miette + thiserror
   ├─ Need structured observability (key-value context, error aggregation)?
   │  └─ Yes → error-stack
   ├─ Need rich debug reports (spantrace, automatic backtrace)?
   │  └─ Yes → eyre + color-eyre
   └─ Default → anyhow
```

### Deprecated / Avoid

| Crate | Status | Replacement |
|-------|--------|-------------|
| `failure` | Unmaintained | `anyhow` + `thiserror` |
| `quick-error` | Unmaintained | `thiserror` |

## Trade-offs

**Structured errors vs Convenience**: Library users get better error handling, but library authors write more boilerplate.

**Context depth**: Too many context layers create noisy logs. Attach context at boundaries only.

**Retry vs Fail-fast**: Retrying improves resilience but adds latency and complexity. Fail-fast is simpler but less resilient.

## Related

- [api-design.md](13-api-design.md) — Error types in public APIs; Rust API Guidelines C-GOOD-ERR
- [toolchain.md](17-toolchain.md) — Clippy rules for error handling
- [errors.md](21-errors.md) — Error combinators and chaining
