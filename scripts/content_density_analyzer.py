#!/usr/bin/env python3
"""
内容密度分析器 — 监控 CrabCache 网关的内容转换效果

分析网关在处理 LLM API 请求过程中，原始请求与转换后请求之间的
"信息密度"和"内容差距"。

用法:
    # 从 Prometheus 指标分析（实时）
    python3 scripts/content_density_analyzer.py --prometheus http://wuming:9090

    # 从 trace JSONL 日志分析（离线）
    python3 scripts/content_density_analyzer.py --trace /var/log/crabcache/trace.jsonl

    # 同时查询两个数据源
    python3 scripts/content_density_analyzer.py \\
        --prometheus http://wuming:9090 \\
        --trace /var/log/crabcache/trace.jsonl
"""

import argparse
import json
import sys
import urllib.request
import urllib.parse
import urllib.error
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Optional


# ═══════════════════════════════════════════════════════════════════════
# Data Classes
# ═══════════════════════════════════════════════════════════════════════

@dataclass
class PipelineDensityStats:
    """Per-pipeline content density statistics."""
    pipeline: str
    total_requests: int = 0
    client_body_bytes: int = 0
    upstream_outbound_bytes: int = 0
    client_outbound_bytes: int = 0
    reasoning_stripped_bytes: int = 0
    reasoning_mirrored_bytes: int = 0
    thinking_block_stripped_bytes: int = 0
    message_retire_est_tokens: int = 0

    @property
    def reasoning_strip_ratio(self) -> float:
        if self.upstream_outbound_bytes == 0:
            return 0.0
        return self.reasoning_stripped_bytes / self.upstream_outbound_bytes

    @property
    def reasoning_mirror_ratio(self) -> float:
        if self.client_outbound_bytes == 0:
            return 0.0
        return self.reasoning_mirrored_bytes / self.client_outbound_bytes


@dataclass
class ContentDensityReport:
    """Full content density analysis report."""
    total_requests: int = 0
    pipelines: dict = field(default_factory=dict)
    total_client_body: int = 0
    total_upstream_outbound: int = 0
    total_client_outbound: int = 0
    total_reasoning_stripped: int = 0
    total_reasoning_mirrored: int = 0
    total_thinking_stripped: int = 0

    def get_pipeline(self, pipeline: str) -> PipelineDensityStats:
        if pipeline not in self.pipelines:
            self.pipelines[pipeline] = PipelineDensityStats(pipeline=pipeline)
        return self.pipelines[pipeline]


# ═══════════════════════════════════════════════════════════════════════
# Prometheus Data Source
# ═══════════════════════════════════════════════════════════════════════

def prom_query(base_url: str, query: str) -> Optional[list]:
    """Execute a Prometheus query via the HTTP API."""
    url = f"{base_url}/api/v1/query?query={urllib.parse.quote(query)}"
    try:
        req = urllib.request.Request(url, headers={"Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
            if data.get("status") == "success":
                return data.get("data", {}).get("result", [])
    except (urllib.error.URLError, json.JSONDecodeError, KeyError) as e:
        print(f"  [WARN] Prometheus query failed: {e}", file=sys.stderr)
    return None


def analyze_prometheus(base_url: str) -> ContentDensityReport:
    """Analyze content density from Prometheus metrics."""
    report = ContentDensityReport()

    print("=" * 70)
    print("Prometheus 内容密度指标查询")
    print("=" * 70)

    # 1. Query content density bytes by stage and pipeline
    print("\n> 查询 gateway_content_density_bytes_total...")
    result = prom_query(base_url, "gateway_content_density_bytes_total")
    if result:
        for item in result:
            labels = item.get("metric", {})
            value = float(item.get("value", [0, "0"])[1])
            stage = labels.get("stage", "?")
            pipeline = labels.get("pipeline", "?")
            stats = report.get_pipeline(pipeline)
            report.total_requests += 1
            if stage == "client_body":
                stats.client_body_bytes += int(value)
                report.total_client_body += int(value)
            elif stage == "upstream_outbound":
                stats.upstream_outbound_bytes += int(value)
                report.total_upstream_outbound += int(value)
            elif stage == "client_outbound":
                stats.client_outbound_bytes += int(value)
                report.total_client_outbound += int(value)
            elif stage == "reasoning_stripped":
                stats.reasoning_stripped_bytes += int(value)
                report.total_reasoning_stripped += int(value)
            elif stage == "reasoning_mirrored":
                stats.reasoning_mirrored_bytes += int(value)
                report.total_reasoning_mirrored += int(value)
    else:
        print("  [INFO] 无数据 (新指标尚未部署或无请求)")

    # 2. Query SSE chunk rewrites
    print("\n> 查询 gateway_sse_chunk_rewrite_total...")
    result = prom_query(base_url, "gateway_sse_chunk_rewrite_total")
    sse_rewrites = defaultdict(lambda: defaultdict(int))
    if result:
        for item in result:
            labels = item.get("metric", {})
            value = int(float(item.get("value", [0, "0"])[1]))
            pipeline = labels.get("pipeline", "?")
            action = labels.get("action", "?")
            sse_rewrites[pipeline][action] = value
    else:
        print("  [INFO] 无数据")

    # 3. Query reasoning store metrics
    print("\n> 查询 gateway_reasoning_store_lookups_total...")
    result = prom_query(base_url, "gateway_reasoning_store_lookups_total")
    reasoning_lookups = {}
    if result:
        for item in result:
            r = item.get("metric", {}).get("result", "?")
            v = int(float(item.get("value", [0, "0"])[1]))
            reasoning_lookups[r] = v

    # 4. Query pipeline selection
    print("\n> 查询 gateway_pipeline_selected_total...")
    result = prom_query(base_url, "gateway_pipeline_selected_total")
    pipeline_selections = defaultdict(int)
    if result:
        for item in result:
            pipeline = item.get("metric", {}).get("pipeline", "?")
            v = int(float(item.get("value", [0, "0"])[1]))
            pipeline_selections[pipeline] += v

    # Print summary
    print("\n" + "=" * 70)
    print("内容密度分析报告 (Prometheus)")
    print("=" * 70)

    if not report.pipelines and not pipeline_selections:
        print("\n[!] 无可用数据。请确保:")
        print("   1. 网关已部署包含 content_density 指标的新版本")
        print("   2. Prometheus 正在抓取网关指标")
        print("   3. 有请求经过网关")
        return report

    # Pipeline selection overview
    if pipeline_selections:
        print("\n> Pipeline 选择分布:")
        total = sum(pipeline_selections.values())
        for p, count in sorted(pipeline_selections.items(), key=lambda x: -x[1]):
            pct = count / total * 100 if total > 0 else 0
            print(f"    {p:30s}  {count:>8,} ({pct:5.1f}%)")

    # Per-pipeline content density
    if report.pipelines:
        print("\n> 各 Pipeline 内容密度:")
        print(f"    {'Pipeline':<30s}  {'Client/Upstream':>16s}  {'Reasoning Strip':>16s}  {'Mirror':>12s}")
        print(f"    {'─' * 30}  {'─' * 16}  {'─' * 16}  {'─' * 12}")
        for p, stats in sorted(report.pipelines.items()):
            if stats.upstream_outbound_bytes > 0:
                ratio_uc = stats.client_outbound_bytes / stats.upstream_outbound_bytes if stats.upstream_outbound_bytes else 0
                strip_pct = stats.reasoning_strip_ratio * 100
                mirror_pct = stats.reasoning_mirror_ratio * 100
                print(f"    {p:<30s}  {ratio_uc:>14.3f}x  {strip_pct:>14.1f}%  {mirror_pct:>10.1f}%")

    # SSE chunk rewrite summary
    if sse_rewrites:
        print("\n> SSE Chunk 改写统计:")
        for p, actions in sorted(sse_rewrites.items()):
            print(f"    {p}:")
            for a, count in sorted(actions.items()):
                print(f"      {a:<25s} {count:>10,}")

    # Reasoning store
    if reasoning_lookups:
        print("\n> ReasoningStore 查询:")
        for r, count in reasoning_lookups.items():
            print(f"    {r:<10s} {count:>10,}")

    # Global summary
    if report.total_upstream_outbound > 0:
        global_strip_ratio = report.total_reasoning_stripped / report.total_upstream_outbound * 100
        global_mirror_ratio = report.total_reasoning_mirrored / report.total_client_outbound * 100 if report.total_client_outbound > 0 else 0
        print(f"\n> 全局汇总:")
        print(f"    客户端请求体总量:     {report.total_client_body:>14,} bytes")
        print(f"    上游发送体总量:       {report.total_upstream_outbound:>14,} bytes")
        print(f"    客户端接收体总量:     {report.total_client_outbound:>14,} bytes")
        print(f"    Reasoning 剥离量:     {report.total_reasoning_stripped:>14,} bytes ({global_strip_ratio:.1f}%)")
        print(f"    Reasoning 镜像量:     {report.total_reasoning_mirrored:>14,} bytes ({global_mirror_ratio:.1f}%)")

    return report


# ═══════════════════════════════════════════════════════════════════════
# Trace JSONL Data Source
# ═══════════════════════════════════════════════════════════════════════

def analyze_trace(trace_path: str, tail_lines: int = 10000) -> ContentDensityReport:
    """Analyze content density from trace JSONL logs."""
    report = ContentDensityReport()

    print("\n" + "=" * 70)
    print("Trace JSONL 内容密度分析")
    print("=" * 70)

    try:
        import subprocess
        result = subprocess.run(
            ["tail", "-n", str(tail_lines), trace_path],
            capture_output=True, text=True, timeout=30
        )
        lines = result.stdout.strip().split("\n") if result.stdout.strip() else []
    except Exception as e:
        print(f"  [ERROR] 无法读取 trace 文件: {e}", file=sys.stderr)
        return report

    if not lines:
        print("  [INFO] trace 文件为空")
        return report

    print(f"  读取最近 {len(lines)} 条 trace 日志...")

    pipeline_stats = defaultdict(lambda: {
        "count": 0,
        "client_body": [],
        "upstream_outbound": [],
        "client_outbound": [],
        "reasoning_stripped": [],
        "reasoning_mirrored": [],
        "thinking_stripped": [],
        "input_tokens": [],
        "output_tokens": [],
        "prompt_tokens": [],
        "cache_hits": 0,
        "cache_misses": 0,
    })

    parse_errors = 0
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            parse_errors += 1
            continue

        report.total_requests += 1
        pipeline = entry.get("pipeline") or entry.get("reasoning_strategy") or "unknown"
        stats = pipeline_stats[pipeline]
        stats["count"] += 1

        cl = entry.get("content_length", 0)
        if cl:
            stats["client_body"].append(cl)

        uob = entry.get("upstream_outbound_bytes", 0)
        if uob:
            stats["upstream_outbound"].append(uob)

        cob = entry.get("client_outbound_bytes", 0)
        if cob:
            stats["client_outbound"].append(cob)

        rsb = entry.get("reasoning_stripped_bytes", 0)
        if rsb:
            stats["reasoning_stripped"].append(rsb)

        rmb = entry.get("reasoning_mirrored_bytes", 0)
        if rmb:
            stats["reasoning_mirrored"].append(rmb)

        tbs = entry.get("thinking_block_stripped_bytes", 0)
        if tbs:
            stats["thinking_stripped"].append(tbs)

        it = entry.get("input_tokens")
        if it:
            stats["input_tokens"].append(it)
        ot = entry.get("output_tokens")
        if ot:
            stats["output_tokens"].append(ot)
        pt = entry.get("prompt_tokens", 0)
        if pt:
            stats["prompt_tokens"].append(pt)

        if entry.get("cache_hit"):
            stats["cache_hits"] += 1
        else:
            stats["cache_misses"] += 1

    if parse_errors:
        print(f"  [WARN] {parse_errors} 行解析失败")

    print(f"\n  成功解析 {report.total_requests} 条 trace 记录\n")

    has_new_fields = False

    for pipeline, stats in sorted(pipeline_stats.items(), key=lambda x: -x[1]["count"]):
        print(f"{'─' * 70}")
        print(f"  Pipeline: {pipeline}")
        print(f"  请求数: {stats['count']:,}")
        print(f"  缓存命中: {stats['cache_hits']:,} / 缓存未命中: {stats['cache_misses']:,}")

        if stats["client_body"]:
            cls = sorted(stats["client_body"])
            avg_cl = sum(cls) / len(cls)
            p50_cl = cls[len(cls) // 2]
            p99_cl = cls[int(len(cls) * 0.99)]
            print(f"  原始请求体: avg={avg_cl:,.0f}B  p50={p50_cl:,}B  p99={p99_cl:,}B")

        if stats["upstream_outbound"]:
            uobs = sorted(stats["upstream_outbound"])
            avg_uob = sum(uobs) / len(uobs)
            p50_uob = uobs[len(uobs) // 2]
            p99_uob = uobs[int(len(uobs) * 0.99)]
            print(f"  上游发送体: avg={avg_uob:,.0f}B  p50={p50_uob:,}B  p99={p99_uob:,}B")

        if stats["client_outbound"]:
            has_new_fields = True
            cobs = sorted(stats["client_outbound"])
            avg_cob = sum(cobs) / len(cobs)
            p50_cob = cobs[len(cobs) // 2]
            p99_cob = cobs[int(len(cobs) * 0.99)]
            print(f"  客户端接收体: avg={avg_cob:,.0f}B  p50={p50_cob:,}B  p99={p99_cob:,}B")

        if stats["upstream_outbound"] and stats["client_outbound"]:
            min_len = min(len(stats["upstream_outbound"]), len(stats["client_outbound"]))
            ratios = [
                stats["client_outbound"][i] / stats["upstream_outbound"][i]
                for i in range(min_len)
                if stats["upstream_outbound"][i] > 0
            ]
            if ratios:
                avg_ratio = sum(ratios) / len(ratios)
                ratios_sorted = sorted(ratios)
                p50_ratio = ratios_sorted[len(ratios_sorted) // 2]
                p99_ratio = ratios_sorted[int(len(ratios_sorted) * 0.99)]
                print(f"  内容密度比 (client/upstream): avg={avg_ratio:.3f}  p50={p50_ratio:.3f}  p99={p99_ratio:.3f}")
                if avg_ratio < 1.0:
                    lost_pct = (1.0 - avg_ratio) * 100
                    print(f"    -> 客户端接收内容比上游少 {lost_pct:.1f}% (reasoning 被剥离)")
                elif avg_ratio > 1.0:
                    extra_pct = (avg_ratio - 1.0) * 100
                    print(f"    -> 客户端接收内容比上游多 {extra_pct:.1f}% (reasoning 被折叠到 content)")

        if stats["reasoning_stripped"]:
            has_new_fields = True
            total_stripped = sum(stats["reasoning_stripped"])
            avg_stripped = total_stripped / len(stats["reasoning_stripped"])
            print(f"  Reasoning 剥离: {total_stripped:,}B 总计, avg={avg_stripped:,.0f}B/请求 ({len(stats['reasoning_stripped'])} 个请求)")

        if stats["reasoning_mirrored"]:
            has_new_fields = True
            total_mirrored = sum(stats["reasoning_mirrored"])
            avg_mirrored = total_mirrored / len(stats["reasoning_mirrored"])
            print(f"  Reasoning 镜像: {total_mirrored:,}B 总计, avg={avg_mirrored:,.0f}B/请求 ({len(stats['reasoning_mirrored'])} 个请求)")

        if stats["thinking_stripped"]:
            has_new_fields = True
            total_thinking = sum(stats["thinking_stripped"])
            avg_thinking = total_thinking / len(stats["thinking_stripped"])
            print(f"  Thinking 块剥离: {total_thinking:,}B 总计, avg={avg_thinking:,.0f}B/请求 ({len(stats['thinking_stripped'])} 个请求)")

        if stats["input_tokens"] and stats["output_tokens"]:
            avg_in = sum(stats["input_tokens"]) / len(stats["input_tokens"])
            avg_out = sum(stats["output_tokens"]) / len(stats["output_tokens"])
            print(f"  Token 统计: avg input={avg_in:,.0f}  avg output={avg_out:,.0f}")

        if stats["prompt_tokens"] and stats["input_tokens"]:
            min_len = min(len(stats["prompt_tokens"]), len(stats["input_tokens"]))
            deltas = [
                stats["prompt_tokens"][i] - stats["input_tokens"][i]
                for i in range(min_len)
            ]
            avg_delta = sum(deltas) / len(deltas) if deltas else 0
            print(f"  Gateway vs 上游 Token 差异: avg={avg_delta:+,.0f} (正值=gateway 预估偏高)")

    # Content gap summary
    print(f"\n{'=' * 70}")
    print("内容差距总结")
    print(f"{'=' * 70}")

    if has_new_fields:
        print("\n新版 trace 日志包含内容密度字段，以下为精确分析:")
        for pipeline, stats in sorted(pipeline_stats.items(), key=lambda x: -x[1]["count"]):
            if stats["upstream_outbound"] and stats["client_outbound"]:
                total_upstream = sum(stats["upstream_outbound"])
                total_client = sum(stats["client_outbound"])
                total_stripped = sum(stats["reasoning_stripped"]) if stats["reasoning_stripped"] else 0
                total_mirrored = sum(stats["reasoning_mirrored"]) if stats["reasoning_mirrored"] else 0
                total_thinking = sum(stats["thinking_stripped"]) if stats["thinking_stripped"] else 0

                print(f"\n  [{pipeline}]")
                if total_upstream > 0:
                    print(f"    上游->客户端 内容保留率: {total_client/total_upstream*100:.1f}%")
                if total_stripped > 0 and total_upstream > 0:
                    print(f"    Reasoning 剥离占比: {total_stripped/total_upstream*100:.1f}% (上游)")
                if total_mirrored > 0 and total_client > 0:
                    print(f"    Reasoning 镜像占比: {total_mirrored/total_client*100:.1f}% (客户端)")
                if total_thinking > 0:
                    print(f"    Thinking 块剥离: {total_thinking:,} bytes")
    else:
        print("\n旧版 trace 日志不包含内容密度字段。")
        print("请部署包含 content_density 追踪的新版本网关后重新分析。")
        print("\n当前可从 content_length 和 upstream_outbound_bytes 推断:")
        for pipeline, stats in sorted(pipeline_stats.items(), key=lambda x: -x[1]["count"]):
            if stats["client_body"] and stats["upstream_outbound"]:
                avg_cl = sum(stats["client_body"]) / len(stats["client_body"])
                avg_uob = sum(stats["upstream_outbound"]) / len(stats["upstream_outbound"])
                delta = avg_uob - avg_cl
                pct = delta / avg_cl * 100 if avg_cl > 0 else 0
                print(f"  [{pipeline}] 请求体->上游: avg {avg_cl:,.0f}B -> {avg_uob:,.0f}B ({pct:+.1f}%)")

    return report


# ═══════════════════════════════════════════════════════════════════════
# Main
# ═══════════════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(
        description="CrabCache 内容密度分析器 - 监控网关内容转换效果"
    )
    parser.add_argument(
        "--prometheus", "-p",
        help="Prometheus 服务地址 (e.g. http://wuming:9090)"
    )
    parser.add_argument(
        "--trace", "-t",
        help="Trace JSONL 日志文件路径"
    )
    parser.add_argument(
        "--tail", "-n",
        type=int, default=10000,
        help="分析 trace 文件的最近 N 行 (默认 10000)"
    )
    args = parser.parse_args()

    if not args.prometheus and not args.trace:
        parser.print_help()
        print("\n请至少指定 --prometheus 或 --trace 参数")
        sys.exit(1)

    reports = []
    if args.prometheus:
        reports.append(analyze_prometheus(args.prometheus))
    if args.trace:
        reports.append(analyze_trace(args.trace, args.tail))

    if len(reports) == 2:
        print(f"\n{'=' * 70}")
        print("数据源交叉对比")
        print(f"{'=' * 70}")
        prom_report, trace_report = reports
        print(f"  Prometheus 报告请求数: {prom_report.total_requests}")
        print(f"  Trace 日志报告请求数:  {trace_report.total_requests}")


if __name__ == "__main__":
    main()
