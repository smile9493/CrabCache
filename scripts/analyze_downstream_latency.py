#!/usr/bin/env python3
"""
Analyze raw_capture/index.jsonl for upstream vs end-to-end latency decomposition.

`prefill_ms` = request start -> upstream response headers (MiMo prefill SLO).
`ttft_ms` = response headers -> first upstream body chunk (SSE TTFT).
`upstream_latency_ms` = upstream response headers -> SSE EOS (generation).
`duration_ms` = request start -> logging (full wall clock).
`gap = duration - upstream` ≈ prefill + upload + client body read (see prefill_ms when present).

Usage:
  python3 scripts/analyze_downstream_latency.py /path/to/index.jsonl
  ssh wuming 'docker exec crabcache-gateway-1 tail -10000 /app/logs/raw_capture/index.jsonl' \\
    | python3 scripts/analyze_downstream_latency.py -
"""

from __future__ import annotations

import json
import statistics as st
import sys


def load_rows(path: str) -> list[dict]:
    rows: list[dict] = []
    src = sys.stdin if path == "-" else open(path, encoding="utf-8")
    with src:
        for line in src:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows


def pct(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    i = min(int(len(s) * p), len(s) - 1)
    return s[i]


def body_bucket(cb: int) -> str:
    if cb < 200_000:
        return "small_lt_200KB"
    if cb < 1_000_000:
        return "medium_200KB_1MB"
    return "large_ge_1MB"


def summarize(name: str, values: list[float]) -> None:
    if not values:
        print(f"{name}: (no samples)")
        return
    print(
        f"{name}: n={len(values)} min={min(values):.0f} p50={pct(values, 0.5):.0f} "
        f"p90={pct(values, 0.9):.0f} max={max(values):.0f} mean={st.mean(values):.0f}"
    )


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2

    path = sys.argv[1]
    all_rows = load_rows(path)
    stream_miss = [
        r
        for r in all_rows
        if r.get("stream") and not r.get("cache_hit") and r.get("upstream_latency_ms") is not None
    ]

    print(f"total lines: {len(all_rows)}")
    print(f"streaming cache-miss with upstream_latency: {len(stream_miss)}")
    if not stream_miss:
        return 1

    stats = []
    for r in stream_miss:
        dur = float(r.get("duration_ms") or 0)
        up = float(r["upstream_latency_ms"])
        gap = dur - up
        cb = int(r.get("client_body_bytes") or 0)
        prefill = r.get("prefill_ms")
        ttft = r.get("ttft_ms")
        stats.append(
            {
                "dur": dur,
                "up": up,
                "gap": gap,
                "prefill": float(prefill) if prefill is not None else None,
                "ttft": float(ttft) if ttft is not None else None,
                "cb": cb,
                "bucket": body_bucket(cb),
                "pipe": r.get("pipeline"),
                "model": r.get("model"),
            }
        )

    gaps = [s["gap"] for s in stats]
    ups = [s["up"] for s in stats]
    durs = [s["dur"] for s in stats]
    prefills = [s["prefill"] for s in stats if s["prefill"] is not None]
    ttfts = [s["ttft"] for s in stats if s["ttft"] is not None]

    print("\n=== Latency breakdown (ms) ===")
    summarize("duration (e2e)", durs)
    summarize("upstream (hdr->EOS)", ups)
    summarize("gap (e2e - upstream)", gaps)
    summarize("prefill (start->hdr)", prefills)
    summarize("sse_ttft (hdr->1st chunk)", ttfts)

    print("\n=== By client body size ===")
    by_bucket: dict[str, list[dict]] = {}
    for s in stats:
        by_bucket.setdefault(s["bucket"], []).append(s)
    for bucket in ("small_lt_200KB", "medium_200KB_1MB", "large_ge_1MB"):
        items = by_bucket.get(bucket, [])
        if not items:
            continue
        print(f"\n{bucket} (n={len(items)})")
        summarize("  e2e", [x["dur"] for x in items])
        summarize("  gap", [x["gap"] for x in items])
        pf = [x["prefill"] for x in items if x["prefill"] is not None]
        if pf:
            summarize("  prefill", pf)

    by_pipe: dict[str, list[dict]] = {}
    for s in stats:
        by_pipe.setdefault(s["pipe"] or "unknown", []).append(s)

    print("\n=== By pipeline ===")
    for pipe, items in sorted(by_pipe.items(), key=lambda x: -len(x[1])):
        g = st.median([x["gap"] for x in items])
        u = st.median([x["up"] for x in items])
        print(f"  {pipe}: n={len(items)}  median_gap={g:.0f}  median_upstream={u:.0f}")

    print("\n=== Top 5 largest gap ===")
    for s in sorted(stats, key=lambda x: -x["gap"])[:5]:
        print(
            f"  dur={s['dur']:.0f} up={s['up']:.0f} gap={s['gap']:.0f} "
            f"prefill={s['prefill']} sse_ttft={s['ttft']} "
            f"body_kb={s['cb'] // 1024} {s['model']} {s['pipe']}"
        )

    print(
        "\nNote: gap mixes body read/upload with prefill; prefer prefill_ms for MiMo SLO. "
        "See docs/OPS_RUNBOOK.md and docs/RUNTIME_LOG_FINDINGS.md."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
