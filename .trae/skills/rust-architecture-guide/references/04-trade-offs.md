# Architectural Trade-offs Framework

## Decision Analysis Framework

Use this framework when facing architectural decisions.

### 1. Identify the Decision

Clearly state the choice:
- State machine vs boolean flags
- Indexing vs smart pointers
- thiserror vs anyhow
- Channels vs shared state

### 2. Assess Context

**Project Type**:
- Library → Prioritize API stability, structured errors
- Application → Prioritize development speed, flexibility

**Performance Criticality**:
- Hot path → Zero-cost abstractions, careful cloning
- Cold path → Developer experience, simple code

**Team Size**:
- Solo → More flexibility, less documentation
- Team → Explicit patterns, shared conventions

### 3. Evaluate Trade-offs

| Dimension | Questions to Ask |
|-----------|-----------------|
| Safety | Does this prevent a class of bugs? |
| Performance | Is this a proven bottleneck? |
| Compile Time | Will this significantly slow builds? |
| Maintainability | Can a new team member understand this? |
| Flexibility | Does this lock us into a pattern? |

### 4. Apply Pragmatic Principle

**Core Question**: Are we pursuing excellence where it matters, or applying dogma everywhere?

**Hot Path / System Boundary** → Invest in optimal solution
**Internal Flow / Cold Path** → Release mental load, keep simple

## Common Trade-off Examples

### State Machine Complexity

**Benefit**: Compile-time state transition guarantees
**Cost**: Boilerplate, learning curve
**Verdict**: Use for core domain entities only

### Newtype Pattern

**Benefit**: Prevents parameter confusion, semantic clarity
**Cost**: Conversion overhead, more types
**Verdict**: Use for IDs and security-sensitive values

### Aggressive Cloning

**Benefit**: Simplifies ownership, reduces lifetime annotations
**Cost**: Runtime allocation, copy overhead
**Verdict**: Fine for small data in cold paths

### Box<dyn Trait>

**Benefit**: Clear API, faster compilation, heterogeneous collections
**Cost**: Heap allocation, dynamic dispatch overhead
**Verdict**: Use when static dispatch becomes complex

## Decision Checklist

Before finalizing any architectural decision:

1. ✅ Does this align with "pragmatism over dogma"?
2. ✅ Have I identified if this is hot path or cold path?
3. ✅ Is this the simplest solution that works?
4. ✅ Can I justify the complexity cost?
5. ✅ Will this scale with team growth?

## Related

- [state-machine.md](07-state-machine.md) — When state machines are worth the cost
- [data-architecture.md](09-data-architecture.md) — Ownership trade-offs
- [error-handling.md](10-error-handling.md) — Error strategy trade-offs
