# Debug Session: gateway-startup-panic
- **Status**: [OPEN]
- **Issue**: `wuming` gateway restarts on boot with panic `there is no reactor running` at `crates/crab-gateway/src/main.rs:868:9`.
- **Debug Server**: http://127.0.0.1:7778/event
- **Log File**: .dbg/trae-debug-log-gateway-startup-panic.ndjson

## Reproduction Steps
1. Hot update gateway to `wuming`.
2. Observe `docker compose ps` shows `gateway` restarting.
3. Observe logs with panic `there is no reactor running, must be called from the context of a Tokio 1.x runtime`.

## Hypotheses & Verification
| ID | Hypothesis | Likelihood | Effort | Evidence |
|----|------------|------------|--------|----------|
| A | Startup path now constructs a Tokio-bound object before entering a runtime | High | Low | Pending |
| B | Panic location line points to nearby field init, but actual culprit is another constructor in the same `GatewayState` block | High | Low | Pending |
| C | Hot update mixed new binary with old config/env, triggering a code path that was previously dormant | Medium | Med | Pending |
| D | Recent debug changes indirectly changed startup order or initialization semantics | Medium | Med | Pending |
| E | Panic is unrelated to this branch and existed in baseline runtime | Low | Low | Pending |

## Log Evidence
- Remote logs show repeated panic: `crates/crab-gateway/src/main.rs:868:9 there is no reactor running`.
- After aligning the old line number with current source, the panic site maps to the rate limiter pruner thread using `tokio::time::interval(...)`.
- The pruner thread is not an async service and does not need Tokio; replacing it with `std::thread::sleep()` removes reactor dependency.

## Verification Conclusion
- Hypothesis A: Confirmed. A startup/background path still depended on Tokio timing APIs.
- Hypothesis B: Confirmed. The logged line in old source mapped to the pruner thread, not `StreamingDeferCircuitBreaker::new()`.
- Applied minimal fix: replace pruner runtime + `tokio::time::interval` with blocking sleep loop.
