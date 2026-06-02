# Debug Session: user-api-key-rate-limit
- **Status**: [OPEN]
- **Issue**: Gateway returns `User API Key Rate limit exceeded`.
- **Debug Server**: pending
- **Log File**: .dbg/trae-debug-log-user-api-key-rate-limit.ndjson

## Reproduction Steps
1. Send request through gateway with the affected user API key.
2. Observe error: `User API Key Rate limit exceeded`.

## Hypotheses & Verification
| ID | Hypothesis | Likelihood | Effort | Evidence |
|----|------------|------------|--------|----------|
| A | Local client-key limiter rejected the request before upstream | High | Low | Pending |
| B | Upstream user-id limiter or quota guard rejected the request | Medium | Low | Pending |
| C | Runtime/config threshold for rate limiting is too low or misconfigured | Medium | Med | Pending |
| D | Recent request burst legitimately exceeded configured per-key limit | High | Low | Pending |
| E | Error body is upstream passthrough rather than local gateway error | Low | Low | Pending |

## Log Evidence
- Local client-key RPM limiter returns `"Rate limit exceeded for this API key..."`, not `"User API Key Rate limit exceeded"`.
- Local DeepSeek user-id concurrency limiter returns `"Too many concurrent DeepSeek requests..."`, not the observed message.
- Docs `CURSOR_SETUP.md` / `OPS_RUNBOOK.md` map the observed message to upstream provider key throttling or WAF.
- Current `wuming` gateway is restarting with panic at `crates/crab-gateway/src/main.rs:868:9`, so live metrics need a stable gateway before verification.

## Verification Conclusion
- Hypothesis A: Rejected by code path/message mismatch.
- Hypothesis B: Rejected by code path/message mismatch.
- Hypothesis D: Plausible but unverified; needs stable runtime metrics.
- Hypothesis E: Provisionally confirmed by docs and message match; likely upstream key/provider throttling surfaced to client.
