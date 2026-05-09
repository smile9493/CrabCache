---
title: "Memory & Lifecycle"
description: "Wasm linear memory lifecycle separation, Arena per-frame lifecycle, physical defense against memory leaks"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["standard", "strict"]
prerequisites: ["01-iron-rules.md"]
dependents: []
---

# Stage 3: Memory & Lifecycle (Linear Memory Lifecycle)

> **Iron Rule Basis**: [IRON-03] Memory Partitioning

Wasm linear memory only grows and cannot return memory to the OS. Memory must be managed through lifecycle partitioning and Arena batch reclamation.

---

## 3.1 Lifecycle Separation (MUST)

Application state must be strictly separated into two lifecycle categories:

**Global Residency (Static/Long-lived)**:

- Core business state machines (e.g., `AppState`) use pre-allocated `Vec`, `arrayvec::ArrayVec`, or `SlotMap`.
- Reside in `static` or `wasm-bindgen` exported root objects, avoid runtime expansion.

**Per-Frame Ephemeral (Transient)**:

- UI rendering intermediate variables, virtual diff trees, etc. must use Arena allocators (e.g., `bumpalo`).
- First line of code on each frame entry: `FRAME_ARENA.with(|a| a.borrow_mut().reset());`
- **Safety Warning**: `bumpalo`'s bump allocation does not guard memory safety by default. Do not store objects with complex `Drop` implementations into the Arena unless their behavior is confirmed.
- **Performance Note**: Arena may not always be faster than heap allocation in certain scenarios. Benchmark with real frame loads is recommended.

---

## 3.2 Physical Defense Against Memory Leaks (MUST)

- When JS components are destroyed, explicitly call the Wasm object's `.free()` method.
- Explore enabling `WeakRef` (ES2021) as an auxiliary means for automated cleanup, but not as the sole dependency.
- Execute global cleanup sequence on page unload event (`beforeunload`).
