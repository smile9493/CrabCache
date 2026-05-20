# pingora-proxy 0.8.0 local patch

Upstream CrabCache reads the full request body in `request_filter` (often >64KiB for Cursor).
Pingora's retry buffer is capped at 64KiB; when truncated, the proxy skipped the initial
`send_body_to_pipe` call, so replaced upstream bodies never reached DeepSeek.

## Changes

- `src/proxy_h1.rs`: also call `send_body_to_pipe` when `session.retry_buffer_truncated()`
- `src/proxy_h2.rs`: same for H2 upstream path

CrabCache `request_body_filter` injects `new_request_body` on that path.
