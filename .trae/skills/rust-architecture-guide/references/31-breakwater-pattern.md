---
title: "Breakwater Architecture Pattern"
description: "Facade/Core layered architecture: ergonomic facade, zero-overhead core, boundary interception protocol"
category: "Architecture"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["07-api-design.md", "09-data-architecture.md"]
dependents: []
---

# Breakwater Architecture Pattern

> **Philosophy**: Unity of False and Real (虚实相生). The Facade is the "false" — ergonomic, forgiving, absorbing chaos. The Core is the "real" — deterministic, zero-overhead, rejecting all invalid state. The boundary between them is the interception layer where semantic translation occurs.

---

## 1. The Pattern: External Input → Facade → Core

```
┌─────────────────────────────────────────────────┐
│                   Facade (虚)                     │
│  Ergonomic API, validation, type contraction      │
│  "Accepts chaos, returns order"                   │
├─────────────────────────────────────────────────┤
│              Boundary Interception                │
│  O(1) conversion, no deep copy, de-oxygenation    │
├─────────────────────────────────────────────────┤
│                    Core (实)                      │
│  Zero-copy processing, no_std倾向, absolute       │
│  determinism, rejects invalid state               │
└─────────────────────────────────────────────────┘
```

**Core Logic**:
- **Facade (虚)**: Extremely ergonomic, tolerant of JS/external input chaos
- **Core (实)**: Kernel-level code pursuing absolute determinism, no illegal state enters
- **Interception**: All external data must complete "de-oxygenation" (type contraction) and validation before entering core

---

## 2. The Boundary Interception Protocol (MUST)

### Rule 1: Core Must Not Depend on External Environment

The core layer must not import `wasm_bindgen`, JS-specific types, or any environment-specific dependencies. Core must maintain `no_std` tendency.

```rust
// ─── Core Layer (lib.rs) ───
// NO wasm_bindgen imports. Pure Rust logic.

mod core_processor;  // Zero-copy, deterministic

pub struct ProcessedData {
    pub value: u64,
    pub metadata: [u8; 32],
}

pub fn process(input: &[u8]) -> Result<ProcessedData, ParseError> {
    // Pure function, no external dependencies
    core_processor::run(input)
}

// ─── Facade Layer (wasm_bindings.rs) ───
use wasm_bindgen::prelude::*;
use crate::core_processor;

#[wasm_bindgen]
pub fn wasm_process(raw: &[u8]) -> Result<JsValue, JsValue> {
    // Facade: validation + conversion
    let result = core_processor::process(raw)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Facade: O(1) conversion to JS
    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
```

### Rule 2: Facade Conversion Must Be O(1) or O(scalar)

Prohibit deep copies in the conversion layer. Use zero-copy views or type contraction.

```rust
// ❌ Prohibited: deep copy in Facade
fn facade_bad(input: JsValue) -> Vec<u8> {
    let parsed: serde_json::Value = input.into_serde().unwrap();
    // Deep serialization/deserialization in boundary
    serde_json::to_vec(&parsed).unwrap()
}

// ✅ Required: O(1) type contraction
fn facade_good(slice: &[u8]) -> &[u8] {
    // Zero-copy view — just narrow the type
    slice
}
```

---

## 3. De-Oxygenation: Type Contraction Protocol

External data is "oxygen-rich" — it carries every possible type, including invalid ones. The Facade must "de-oxygenate" before handing to Core.

```rust
// External input: rich, chaotic, potentially invalid
pub enum RawCommand {
    Move { x: i32, y: i32 },
    Resize { width: u32, height: u32 },
    Invalid { garbage: Vec<u8> },
    Unknown { data: String },
}

// After de-oxygenation: contracted, validated, Core-ready
pub enum Command {
    Move { x: i32, y: i32 },
    Resize { width: NonZeroU32, height: NonZeroU32 },
}

// Facade: the interception layer
impl TryFrom<RawCommand> for Command {
    type Error = ValidationError;

    fn try_from(raw: RawCommand) -> Result<Self, Self::Error> {
        match raw {
            RawCommand::Move { x, y } => Ok(Self::Move { x, y }),
            RawCommand::Resize { width, height } => {
                let width = NonZeroU32::try_from(width)
                    .map_err(|_| ValidationError::ZeroWidth)?;
                let height = NonZeroU32::try_from(height)
                    .map_err(|_| ValidationError::ZeroHeight)?;
                Ok(Self::Resize { width, height })
            }
            RawCommand::Invalid { .. } => Err(ValidationError::InvalidCommand),
            RawCommand::Unknown { .. } => Err(ValidationError::UnknownCommand),
        }
    }
}
```

---

## 4. The Three-Layer Architecture: Full Example

```rust
// ═══════════════════════════════════════════
// Layer 1: External Input (Chaos)
// ═══════════════════════════════════════════
// Raw bytes from WASM/Network/FFI — no guarantees

// ═══════════════════════════════════════════
// Layer 2: Facade (虚) — Validation + Contraction
// ═══════════════════════════════════════════

pub struct RequestFacade {
    raw: Vec<u8>,  // O(1) ownership transfer from external
}

impl RequestFacade {
    pub fn validate(&self) -> Result<ValidatedRequest, ValidationError> {
        // Parse header (zero-copy view)
        let header = self.raw.get(..16)
            .ok_or(ValidationError::TooShort)?;

        // Type contraction: extract only what Core needs
        let version = header[0];
        if version != 1 {
            return Err(ValidationError::UnsupportedVersion(version));
        }

        let payload_len = u32::from_le_bytes(header[12..16].try_into().unwrap());
        if payload_len > 1024 * 1024 {
            return Err(ValidationError::PayloadTooLarge);
        }

        Ok(ValidatedRequest {
            version,
            payload: &self.raw[16..],
        })
    }
}

// ═══════════════════════════════════════════
// Layer 3: Core (实) — Zero-Copy Processing
// ═══════════════════════════════════════════

pub struct ValidatedRequest<'a> {
    pub version: u8,
    pub payload: &'a [u8],
}

impl ValidatedRequest<'_> {
    pub fn process(&self) -> Result<Response, ProcessingError> {
        // Pure, deterministic logic
        // No external dependencies
        // All invariants guaranteed by Facade
        crate::core_logic::execute(self.payload)
    }
}
```

---

## 5. Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| Core imports `wasm_bindgen` | Core coupled to JS environment | Move all `#[wasm_bindgen]` to separate facade module |
| Facade does deep copy | O(n) conversion at boundary | Use zero-copy views or type contraction |
| No validation before Core | Invalid state reaches core | Facade must validate all external input |
| Facade is too thin | Validation leaks into Core | Facade absorbs all parsing/conversion complexity |
| Facade is too thick | Core logic leaks into Facade | Keep Core pure, Facade is only translation |

---

## 6. Philosophy: Why "Breakwater"?

A breakwater absorbs the chaotic energy of waves (external input), transforming it into calm water (validated input) that the harbor (Core) can safely process. The breakwater itself must be strong enough to absorb the impact, but thin enough to not slow the flow.

This is the **Jeet Kune Do interception principle**: intercept errors at the boundary, before they reach the core. The Facade is the extended arm that absorbs and redirects; the Core is the fist that strikes with certainty.
