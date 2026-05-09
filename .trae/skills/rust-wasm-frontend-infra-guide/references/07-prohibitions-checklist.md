---
title: "Prohibitions & Compliance Self-Check"
description: "7 hard prohibitions and 10-item compliance self-check list"
category: "Compliance"
priority: "P0"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md", "02-build-control.md", "03-ffi-boundary.md", "04-memory-lifecycle.md", "05-concurrency-events.md", "06-wasm-adaptation.md"]
dependents: []
---

# Prohibitions & Compliance Self-Check

---

## 6. Prohibition List (Hard Forbidden)

- **[F-01]** Must not use `std::thread::sleep` or blocking `Mutex::lock()` under the `wasm32-unknown-unknown` target.
- **[F-02]** Must not call `serde_json` or any serialization library on hot FFI paths.
- **[F-03]** Must not execute `Box::new` or `Vec::push` (triggering heap allocation) within the frame loop unless in a globally pre-allocated container.
- **[F-04]** Must not use `println!` for production logging; must use `tracing` + `console_error_panic_hook`.
- **[F-05]** Must not depend on the Wasm GC proposal (reference types, etc.) until the Wasm 2.0 specification lands and mainstream browsers support it.
- **[F-06]** Must not select `wee_alloc` as the allocator for new projects (no longer maintained).
- **[F-07]** Must not claim `SharedArrayBuffer` support without documented COOP/COEP configuration requirements.

---

## 7. Compliance Self-Check List

- [ ] `[profile.release]` in `Cargo.toml` includes `opt-level="z"`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true`.
- [ ] CI pipeline integrates `wasm-opt -Oz` step with file size regression check.
- [ ] Allocator replaced from default to `talc` or `MiniAlloc` (`wee_alloc` prohibited), volume benchmark completed.
- [ ] All cross-boundary large data paths use zero-copy view encapsulation with lifetimes explicitly documented.
- [ ] FFI interfaces use only scalar parameters or `WasmSlice`, no `JsValue` pass-through.
- [ ] First line of frame loop entry is Arena `reset()`.
- [ ] Async I/O uses only `wasm-bindgen-futures`, no blocking calls.
- [ ] If `SharedArrayBuffer` is enabled, COOP/COEP header configuration is documented in architecture decision record.
- [ ] Initialization code calls `console_error_panic_hook::set_once()` and `tracing_wasm::set_as_global_default()`.
- [ ] Error handling uses `Result<T, JsValue>` pattern, no misuse of `panic!` for business logic.
