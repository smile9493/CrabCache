# Debug Session: incomplete-body-upstream
- **Status**: [OPEN]
- **Issue**: Provider returns `incomplete_body` after streaming defer arm/finalize changes.
- **Debug Server**: http://127.0.0.1:7777/event
- **Log File**: .dbg/trae-debug-log-incomplete-body-upstream.ndjson

## Reproduction Steps
1. Heat update gateway to `wuming`.
2. Send MiMo request large enough to hit streaming defer path.
3. Observe provider error: `Request body incomplete (client upload truncated or proxy buffering)`.

## Hypotheses & Verification
| ID | Hypothesis | Likelihood | Effort | Evidence |
|----|------------|------------|--------|----------|
| A | Defer arm now starts too early, but EOS finalize sends fewer bytes than declared `Content-Length` | High | Low | Pending |
| B | Prepared upstream body is emitted twice or split incorrectly across defer/finalize, so provider sees malformed framing | High | Med | Pending |
| C | Upstream request headers still carry stale `Content-Length` or chunked semantics after body rewrite | Medium | Med | Pending |
| D | Empty EOS suppression or trailing chunk handling drops the last chunk on defer path | Medium | Med | Pending |
| E | Error comes from old/non-defer traffic and is unrelated to the recent arm gating change | Low | Low | Pending |

## Log Evidence
- Added instrumentation points:
  - `A`: defer arm state
  - `B`: finalized assembled body length
  - `C`: defer upstream framing headers
  - `D`: prepared body emission at EOS
- `request_id=477719c2-34af-4644-bdfe-e8407475d04c`
  - defer armed: `partial_len=32768`, `inbound_content_length=333084`
  - finalized body: `assembled_len=32768`, `append_tail_bytes=0`
  - upstream error: `Peer prematurely closed connection with 289651 bytes of body remaining to read`
- `request_id=c1b9f79e-7576-4a2d-8d8f-f9fae3dbfb53`
  - defer armed: `partial_len=81920`, `inbound_content_length=201322`
  - finalized body: `assembled_len=81920`, `append_tail_bytes=0`
  - upstream error: `Peer prematurely closed connection with 119402 bytes of body remaining to read`
- Defer upstream headers omit both `Content-Length` and `Transfer-Encoding`, so provider expects the remaining body to arrive after headers.
- In both samples, no post-arm tail chunk arrived before finalize, so gateway emitted only the deferred partial body.

## Verification Conclusion
- Hypothesis A: Confirmed. Upstream receives only a prefix of the client body.
- Hypothesis B: Rejected. No evidence of duplicate prepared-body emission.
- Hypothesis C: Rejected. Header framing matches defer-mode intent; the failure is body truncation.
- Hypothesis D: Confirmed in effect. Finalize occurs with `append_tail_bytes=0`, so the remaining downstream body never reaches upstream.
- Hypothesis E: Rejected. Both failures are from new defer-path requests after gateway recovery.
- Applied minimal fix: reject streaming defer for `stream=false` requests with reason `non_streaming_request`, sending them back to the full-body path.
