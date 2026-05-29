#!/usr/bin/env python3
"""
Unified CrabCache gateway log analyzer (trace + docker logs + Prometheus).

Covers the common post-deploy / incident checks:
  - MiMo prefill / upstream / e2e latency from trace.jsonl or raw_capture/index.jsonl
  - Body-size buckets, pipeline breakdown, top slow requests
  - Gateway docker log health: JSON parse_fail, streaming_defer, panic, Redis bb8, ConnectionClosed
  - Prometheus defer counters and phase latency (when metrics port reachable)

Examples:
  # Full report from wuming (SSH fetch trace tail, docker logs, metrics)
  python3 scripts/analyze_gateway_logs.py --target wuming

  # Local trace file, last 200 lines
  python3 scripts/analyze_gateway_logs.py --trace /path/to/trace.jsonl --tail 200

  # Compare recent N lines vs previous N lines (before/after hot-update)
  python3 scripts/analyze_gateway_logs.py --trace trace.jsonl --tail 100 --compare-tail 100

  # Docker logs from stdin
  ssh wuming 'docker logs crabcache-gateway-1 --tail 500 2>&1' \\
    | python3 scripts/analyze_gateway_logs.py --docker-logs -

  # raw_capture index (same fields as analyze_downstream_latency.py)
  python3 scripts/analyze_gateway_logs.py --raw /app/logs/raw_capture/index.jsonl --target wuming
"""

from __future__ import annotations

import argparse
import json
import re
import statistics as st
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from io import StringIO
from typing import Any, Iterable, TextIO

DEPLOY_TARGETS = {
    "crabcache-deploy": {"ssh": "crabcache-deploy"},
    "wuming": {"ssh": "wuming"},
}

GATEWAY_CONTAINER = "crabcache-gateway-1"
TRACE_PATH = "/app/logs/trace.jsonl"
RAW_CAPTURE_PATH = "/app/logs/raw_capture/index.jsonl"
METRICS_URL = "http://127.0.0.1:9090/metrics"

# Docker log line: 2026-05-29T12:40:06.181680Z LEVEL module: message key=val ...
LOG_LINE_RE = re.compile(
    r"^(?P<ts>\d{4}-\d{2}-\d{2}T[\d:.]+Z)\s+"
    r"(?P<level>\w+)\s+"
    r"(?P<module>\S+):\s+"
    r"(?P<msg>.*?)"
    r"(?:\s+(?P<fields>.*))?$"
)
KV_RE = re.compile(r"(\w+)=([^\s]+)")


@dataclass
class LatencyStats:
    name: str
    values: list[float] = field(default_factory=list)

    def add(self, v: float | int | None) -> None:
        if v is not None:
            self.values.append(float(v))

    def summary(self) -> str:
        if not self.values:
            return f"{self.name}: (no samples)"
        s = sorted(self.values)
        p50 = s[min(int(len(s) * 0.5), len(s) - 1)]
        p90 = s[min(int(len(s) * 0.9), len(s) - 1)]
        return (
            f"{self.name}: n={len(s)} min={min(s):.0f} p50={p50:.0f} "
            f"p90={p90:.0f} max={max(s):.0f} mean={st.mean(s):.0f}"
        )


def load_jsonl(source: str | TextIO, *, tail: int | None = None) -> list[dict]:
    rows: list[dict] = []
    handle: TextIO
    if isinstance(source, str):
        if source == "-":
            handle = sys.stdin
        else:
            handle = open(source, encoding="utf-8")
    else:
        handle = source
    with handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    if tail is not None and tail > 0 and len(rows) > tail:
        rows = rows[-tail:]
    return rows


def body_bucket(content_length: int) -> str:
    if content_length < 200_000:
        return "lt_200KB"
    if content_length < 1_000_000:
        return "200KB_1MB"
    return "ge_1MB"


def ssh_run(host: str, cmd: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["ssh", host, cmd],
        text=True,
        capture_output=True,
    )


def fetch_remote_text(host: str, cmd: str) -> str:
    proc = ssh_run(host, cmd)
    if proc.returncode != 0:
        raise RuntimeError(f"ssh {host} failed: {proc.stderr or proc.stdout}")
    return proc.stdout or ""


def analyze_trace_rows(rows: list[dict], *, label: str = "trace") -> None:
    if not rows:
        print(f"\n=== {label}: no records ===")
        return

    miss = [r for r in rows if not r.get("cache_hit")]
    stream_miss = [
        r
        for r in miss
        if r.get("upstream_latency_ms") is not None or r.get("prefill_ms") is not None
    ]

    print(f"\n=== {label}: latency ({len(stream_miss)} cache-miss with timing / {len(rows)} total) ===")

    prefill = LatencyStats("prefill (start->hdr)")
    upstream = LatencyStats("upstream (hdr->EOS)")
    e2e = LatencyStats("e2e (latency_ms)")
    ttft = LatencyStats("sse_ttft (hdr->1st chunk)")
    gap = LatencyStats("gap (e2e - upstream)")

    by_pipe: dict[str, list[dict]] = defaultdict(list)
    by_bucket: dict[str, list[dict]] = defaultdict(list)

    for r in stream_miss:
        dur = float(r.get("latency_ms") or 0)
        up = r.get("upstream_latency_ms")
        pf = r.get("prefill_ms") or r.get("pre_header_ms")
        e2e.add(dur)
        prefill.add(pf)
        upstream.add(up)
        ttft.add(r.get("ttft_ms"))
        if up is not None:
            gap.add(dur - float(up))
        pipe = r.get("pipeline") or "unknown"
        by_pipe[pipe].append(r)
        by_bucket[body_bucket(int(r.get("content_length") or 0))].append(r)

    for stat in (e2e, prefill, upstream, gap, ttft):
        print(stat.summary())

    print("\n--- by pipeline (median prefill / upstream / e2e) ---")
    for pipe, items in sorted(by_pipe.items(), key=lambda x: -len(x[1])):
        pf = [float(x["prefill_ms"]) for x in items if x.get("prefill_ms") is not None]
        up = [float(x["upstream_latency_ms"]) for x in items if x.get("upstream_latency_ms") is not None]
        du = [float(x["latency_ms"]) for x in items if x.get("latency_ms") is not None]
        med_pf = st.median(pf) if pf else 0
        med_up = st.median(up) if up else 0
        med_du = st.median(du) if du else 0
        print(f"  {pipe}: n={len(items)}  prefill_p50={med_pf:.0f}  upstream_p50={med_up:.0f}  e2e_p50={med_du:.0f}")

    print("\n--- by body size ---")
    for bucket in ("lt_200KB", "200KB_1MB", "ge_1MB"):
        items = by_bucket.get(bucket, [])
        if not items:
            continue
        pf = [float(x["prefill_ms"]) for x in items if x.get("prefill_ms") is not None]
        up = [float(x["upstream_latency_ms"]) for x in items if x.get("upstream_latency_ms") is not None]
        pf_med = st.median(pf) if pf else 0
        up_med = st.median(up) if up else 0
        print(
            f"  {bucket}: n={len(items)}  "
            f"prefill_p50={pf_med:.0f}  upstream_p50={up_med:.0f}"
        )

    print("\n--- top 5 slowest prefill ---")
    ranked = sorted(
        stream_miss,
        key=lambda r: float(r.get("prefill_ms") or r.get("pre_header_ms") or 0),
        reverse=True,
    )[:5]
    for r in ranked:
        pf = r.get("prefill_ms") or r.get("pre_header_ms")
        print(
            f"  prefill={pf:.0f} upstream={r.get('upstream_latency_ms')} e2e={r.get('latency_ms')} "
            f"body_kb={int(r.get('content_length') or 0) // 1024} "
            f"model={r.get('model')} pipeline={r.get('pipeline')} "
            f"hash={str(r.get('request_hash', ''))[:12]}"
        )


@dataclass
class LogHealth:
    parse_fail: list[dict] = field(default_factory=list)
    defer_parse_fail: list[dict] = field(default_factory=list)
    panic: list[str] = field(default_factory=list)
    redis_timeout: list[str] = field(default_factory=list)
    connection_closed: list[dict] = field(default_factory=list)
    upstream_400: list[dict] = field(default_factory=list)
    other_errors: Counter[str] = field(default_factory=Counter)


def parse_docker_log_line(line: str) -> dict[str, Any] | None:
    # Strip ANSI
    line = re.sub(r"\x1b\[[0-9;]*m", "", line.strip())
    if not line:
        return None
    m = LOG_LINE_RE.match(line)
    if not m:
        return None
    fields: dict[str, str] = {}
    if m.group("fields"):
        fields = dict(KV_RE.findall(m.group("fields")))
    return {
        "ts": m.group("ts"),
        "level": m.group("level"),
        "module": m.group("module"),
        "msg": m.group("msg"),
        "fields": fields,
        "raw": line,
    }


def analyze_docker_logs(text: str, *, label: str = "docker logs") -> LogHealth:
    health = LogHealth()
    for line in text.splitlines():
        entry = parse_docker_log_line(line)
        if not entry:
            continue
        msg = entry["msg"]
        fields = entry["fields"]
        level = entry["level"]
        module = entry["module"]

        if "Rejecting request: client JSON parse failed" in msg:
            rec = {**fields, "ts": entry["ts"], "msg": msg}
            health.parse_fail.append(rec)
            if fields.get("streaming_defer") == "true":
                health.defer_parse_fail.append(rec)
        elif "Panic occurred" in msg or (module == "crab_gateway" and "Panic" in msg):
            health.panic.append(entry["raw"])
        elif "Timed out in bb8" in msg or "Failed to persist control plane state to Redis" in msg:
            health.redis_timeout.append(entry["raw"])
        elif "Fail to proxy: Downstream ConnectionClosed" in msg:
            health.connection_closed.append(
                {"ts": entry["ts"], "msg": msg, "request_id": fields.get("request_id")}
            )
        elif "Upstream returned error status" in msg and fields.get("status") == "400":
            health.upstream_400.append({**fields, "ts": entry["ts"]})
        elif level == "ERROR":
            key = f"{module}: {msg[:80]}"
            health.other_errors[key] += 1

    print(f"\n=== {label}: health scan ===")
    print(f"  JSON parse_fail total:        {len(health.parse_fail)}")
    print(f"    └ streaming_defer=true:     {len(health.defer_parse_fail)}")
    print(f"  Tokio panic / crash:          {len(health.panic)}")
    print(f"  Redis bb8 persist timeout:    {len(health.redis_timeout)}")
    print(f"  Downstream ConnectionClosed:   {len(health.connection_closed)}")
    print(f"  Upstream HTTP 400:            {len(health.upstream_400)}")

    if health.defer_parse_fail:
        print("\n  defer parse_fail samples:")
        for r in health.defer_parse_fail[-3:]:
            print(
                f"    {r.get('ts')} request_id={r.get('request_id')} "
                f"body_len={r.get('body_len')} parse_error={r.get('parse_error')}"
            )
    elif health.parse_fail:
        print("\n  parse_fail samples (non-defer):")
        for r in health.parse_fail[-3:]:
            print(
                f"    {r.get('ts')} request_id={r.get('request_id')} "
                f"body_len={r.get('body_len')} defer={r.get('streaming_defer', 'n/a')}"
            )

    if health.panic:
        print("\n  panic samples:")
        for line in health.panic[-2:]:
            print(f"    {line[:160]}")

    if health.other_errors:
        print("\n  other ERROR (top 5):")
        for key, cnt in health.other_errors.most_common(5):
            print(f"    [{cnt}x] {key}")

    return health


def parse_prometheus_counters(text: str, prefix: str) -> dict[str, float]:
    out: dict[str, float] = {}
    for line in text.splitlines():
        if not line.startswith(prefix) or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) >= 2:
            try:
                out[parts[0]] = float(parts[1])
            except ValueError:
                continue
    return out


def analyze_metrics(text: str) -> None:
    print("\n=== Prometheus: streaming defer counters ===")
    defer = parse_prometheus_counters(text, "gateway_streaming_defer_")
    if not defer:
        print("  (no gateway_streaming_defer_* counters — process may have restarted)")
    for name in sorted(defer):
        short = name.replace("gateway_streaming_defer_", "")
        print(f"  {short}: {defer[name]:.0f}")

    print("\n=== Prometheus: phase latency (sum/count → avg ms) ===")
    sums = parse_prometheus_counters(text, "gateway_request_phase_latency_seconds_sum")
    counts = parse_prometheus_counters(text, "gateway_request_phase_latency_seconds_count")
    by_model_phase: dict[tuple[str, str], tuple[float, float]] = {}
    for key, total in sums.items():
        # gateway_request_phase_latency_seconds_sum{model="mimo-v2.5-pro",phase="prefill_done",pipeline="mimo_relay"}
        m = re.search(r'model="([^"]+)".*phase="([^"]+)"', key)
        if not m:
            continue
        model, phase = m.group(1), m.group(2)
        cnt_key = key.replace("_sum", "_count")
        cnt = counts.get(cnt_key, 0)
        if cnt > 0:
            by_model_phase[(model, phase)] = (total / cnt * 1000, cnt)

    for model in sorted({k[0] for k in by_model_phase}):
        print(f"  [{model}]")
        phases = ["body_read_done", "json_parse_done", "upstream_connect_done", "prefill_done", "ttft"]
        for phase in phases:
            val = by_model_phase.get((model, phase))
            if val:
                avg_ms, n = val
                print(f"    {phase}: avg={avg_ms:.1f}ms (n={n:.0f})")

    print("\n=== Prometheus: upstream latency avg ===")
    up_sum = parse_prometheus_counters(text, "gateway_upstream_latency_seconds_sum")
    up_cnt = parse_prometheus_counters(text, "gateway_upstream_latency_seconds_count")
    for key, total in up_sum.items():
        m = re.search(r'model="([^"]+)"', key)
        if not m:
            continue
        model = m.group(1)
        cnt = up_cnt.get(key.replace("_sum", "_count"), 0)
        if cnt > 0:
            print(f"  {model}: avg={total / cnt * 1000:.0f}ms (n={cnt:.0f})")


def compare_trace_windows(recent: list[dict], older: list[dict]) -> None:
    print("\n=== Before / after comparison (older vs recent tail) ===")

    def med_pf(rows: list[dict]) -> float | None:
        vals = [float(r["prefill_ms"]) for r in rows if r.get("prefill_ms") is not None]
        return st.median(vals) if vals else None

    def med_up(rows: list[dict]) -> float | None:
        vals = [float(r["upstream_latency_ms"]) for r in rows if r.get("upstream_latency_ms") is not None]
        return st.median(vals) if vals else None

    o_pf, r_pf = med_pf(older), med_pf(recent)
    o_up, r_up = med_up(older), med_up(recent)
    print(f"  samples:  older={len(older)}  recent={len(recent)}")
    if o_pf is not None and r_pf is not None:
        delta = r_pf - o_pf
        print(f"  prefill_p50:  {o_pf:.0f} -> {r_pf:.0f} ms  ({delta:+.0f})")
    if o_up is not None and r_up is not None:
        delta = r_up - o_up
        print(f"  upstream_p50: {o_up:.0f} -> {r_up:.0f} ms  ({delta:+.0f})")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="CrabCache gateway log analyzer (trace + docker logs + metrics)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument(
        "--target",
        choices=sorted(DEPLOY_TARGETS),
        help="SSH deploy target; fetches trace, docker logs, metrics from remote gateway",
    )
    p.add_argument("--trace", metavar="PATH", help="trace.jsonl path (- for stdin)")
    p.add_argument("--raw", metavar="PATH", help="raw_capture/index.jsonl path")
    p.add_argument("--docker-logs", metavar="PATH", help="gateway docker log text (- for stdin)")
    p.add_argument("--metrics", metavar="PATH", help="Prometheus metrics text (- for stdin)")
    p.add_argument("--tail", type=int, default=500, help="Use last N lines for trace/raw (default 500)")
    p.add_argument(
        "--compare-tail",
        type=int,
        metavar="N",
        help="Compare last N trace lines vs previous N (before/after hot-update)",
    )
    p.add_argument(
        "--log-tail",
        type=int,
        default=800,
        help="Docker log lines when fetching remotely (default 800)",
    )
    p.add_argument(
        "--since",
        metavar="ISO",
        help="Only docker log lines at or after timestamp (e.g. 2026-05-29T12:48:00Z)",
    )
    return p


def filter_since(text: str, since: str | None) -> str:
    if not since:
        return text
    lines = []
    for line in text.splitlines():
        entry = parse_docker_log_line(re.sub(r"\x1b\[[0-9;]*m", "", line.strip()))
        if entry and entry["ts"] >= since:
            lines.append(line)
    return "\n".join(lines)


def main() -> int:
    args = build_parser().parse_args()
    host = DEPLOY_TARGETS[args.target]["ssh"] if args.target else None

    print(f"CrabCache gateway log analysis  {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}")
    if args.target:
        print(f"  target: {args.target} ({host})")

    trace_rows: list[dict] | None = None
    log_text: str | None = None
    metrics_text: str | None = None

    # --- fetch or load trace ---
    if args.trace:
        trace_rows = load_jsonl(args.trace, tail=args.tail)
    elif args.raw:
        trace_rows = load_jsonl(args.raw, tail=args.tail)
    elif host:
        path = RAW_CAPTURE_PATH if args.raw is not None else TRACE_PATH
        cmd = f"docker exec {GATEWAY_CONTAINER} tail -n {args.tail} {path} 2>/dev/null"
        trace_rows = load_jsonl(StringIO(fetch_remote_text(host, cmd)), tail=args.tail)

    # --- fetch or load docker logs ---
    if args.docker_logs:
        src = sys.stdin if args.docker_logs == "-" else open(args.docker_logs, encoding="utf-8")
        with src:
            log_text = src.read()
    elif host and not args.trace and not args.metrics:
        cmd = f"docker logs {GATEWAY_CONTAINER} --tail {args.log_tail} 2>&1"
        log_text = fetch_remote_text(host, cmd)

    # --- fetch or load metrics ---
    if args.metrics:
        src = sys.stdin if args.metrics == "-" else open(args.metrics, encoding="utf-8")
        with src:
            metrics_text = src.read()
    elif host:
        proc = ssh_run(host, f"curl -sf {METRICS_URL} 2>/dev/null || true")
        metrics_text = proc.stdout or ""

    if log_text is not None:
        log_text = filter_since(log_text, args.since)

    # --- analyze ---
    if trace_rows is not None:
        if args.compare_tail and len(trace_rows) >= args.compare_tail * 2:
            split = len(trace_rows) - args.compare_tail
            compare_trace_windows(trace_rows[split:], trace_rows[:split])
        label = "raw_capture" if args.raw else "trace"
        analyze_trace_rows(trace_rows, label=label)

    if log_text:
        analyze_docker_logs(log_text)

    if metrics_text:
        analyze_metrics(metrics_text)

    if trace_rows is None and log_text is None and metrics_text is None:
        print("\nNo input. Use --target wuming or pass --trace / --docker-logs / --metrics.", file=sys.stderr)
        return 2

    print("\n--- hints ---")
    print("  prefill_ms = request start -> upstream response headers (MiMo SLO)")
    print("  gap ≈ body read + upload + prefill; prefer prefill_ms over gap")
    print("  defer parse_fail with streaming_defer=true → see docs/STREAMING_BODY_FORWARD.md")
    print("  full raw_capture: python3 scripts/analyze_downstream_latency.py -")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
