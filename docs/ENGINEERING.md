# Engineering Guide

This document records the public-safe engineering workflow for CrabCache. Keep
private deployment hosts, SSH aliases, internal IPs, and secrets in local-only
notes such as `AGENTS.md`.

## Local Checks

Run the same checks locally before pushing reviewed changes:

```bash
make presubmit
```

`make presubmit` runs formatting, clippy, cargo-deny, core crate tests, and a
WASM compile check for the dashboard. It does not rewrite files. If formatting
fails, run `cargo fmt --all` yourself and rerun the presubmit.

## Dashboard Builds

Build Admin dashboard assets with:

```bash
make build-dashboard
```

The build writes `crates/crab-dashboard/dist/build-info.json` with a git commit,
asset hashes, and a `dashboard_dist_hash`. The Admin version endpoint exposes
that metadata so deploy checks can detect a stale or mismatched dashboard dist.

## Deployment

Build locally and hot-update remote containers with:

```bash
make hot-update
```

Remote deployment hosts must only run Docker lifecycle commands. Do not run
`cargo build`, `cargo test`, Trunk, Rustup, or frontend build commands on remote
deployment machines.

When Admin or Dashboard code changes, update both the `crab-admin` binary and
`crates/crab-dashboard/dist`. Updating only one side can cause Admin API paths
to return stale SPA HTML instead of JSON.

## Verification

For a local running stack, use:

```bash
make verify-local
```

Set `CLIENT_API_KEY` to include the full chat-completion smoke test. Without it,
the script still checks gateway readiness and the Admin JSON boundary.

After hot-update, confirm:

- Gateway readiness returns OK.
- Admin homepage serves the dashboard.
- Admin API probes return `application/json`, never SPA `text/html`.
- The remote dashboard `dashboard_dist_hash` matches the local build.

## Public Release Hygiene

Before pushing to a public remote, confirm local-only files and secrets are not
tracked:

```bash
git status
git ls-files AGENTS.md .env
```

Do not commit `.env`, credentials, private deployment hostnames, internal IPs,
or SSH configuration.
