#!/usr/bin/env python3
"""
Trace Analyzer - Analyze collected traces and fit simulation parameters.

Usage:
    python analyze_trace.py trace.jsonl
    
The trace file should contain JSON lines from the shadow log, each with:
    - timestamp_ms: Unix timestamp in milliseconds
    - request_hash: SHA256 hash (first 16 chars) of the request body
    - content_length: Length of the request body in bytes
    - semantic_cluster: Semantic cluster bucket (0-99)
    - conversation_id: Optional conversation identifier
    - model: Model name
    - prompt_tokens: Number of prompt tokens
    - latency_ms: Request latency in milliseconds
    - cache_hit: Boolean indicating if cache was hit
"""

import json
import sys
import math
from collections import Counter, defaultdict
from dataclasses import dataclass
from typing import List, Optional


@dataclass
class FittedParams:
    total_requests: int
    unique_requests: int
    repeat_ratio: float
    semantic_cluster_ratio: float
    estimated_zipf_alpha: float
    conversation_ratio: float
    avg_latency_ms: float
    cache_hit_rate: float
    total_tokens: int
    avg_prompt_tokens: float


def load_trace(path: str) -> List[dict]:
    records = []
    with open(path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
                records.append(record)
            except json.JSONDecodeError:
                continue
    return records


def zipf_alpha(counts: List[int]) -> float:
    if len(counts) < 2:
        return 0.0
    
    ranks = list(range(1, len(counts) + 1))
    log_ranks = [math.log(r) for r in ranks]
    log_freqs = [math.log(f) for f in counts]
    
    n = len(log_ranks)
    sum_x = sum(log_ranks)
    sum_y = sum(log_freqs)
    sum_xy = sum(x * y for x, y in zip(log_ranks, log_freqs))
    sum_xx = sum(x * x for x in log_ranks)
    
    denominator = n * sum_xx - sum_x * sum_x
    if denominator == 0:
        return 0.0
    
    alpha = (n * sum_xy - sum_x * sum_y) / denominator
    return -alpha


def analyze_trace(records: List[dict]) -> FittedParams:
    if not records:
        return FittedParams(0, 0, 0, 0, 1.0, 0, 0, 0, 0, 0)
    
    total = len(records)
    
    hashes = [r.get('request_hash', 'unknown') for r in records]
    unique_hashes = set(hashes)
    unique = len(unique_hashes)
    
    repeat_ratio = 1.0 - (unique / total) if total > 0 else 0
    
    freq = Counter(hashes)
    counts = sorted(freq.values(), reverse=True)
    
    alpha = zipf_alpha(counts)
    
    semantic_clusters = [r.get('semantic_cluster') for r in records if r.get('semantic_cluster') is not None]
    if semantic_clusters:
        cluster_counts = Counter(semantic_clusters)
        multi_member = sum(c for c in cluster_counts.values() if c > 1)
        semantic_ratio = multi_member / len(semantic_clusters)
    else:
        semantic_ratio = 0.0
    
    with_conv = sum(1 for r in records if r.get('conversation_id'))
    conversation_ratio = with_conv / total if total > 0 else 0
    
    latencies = [r.get('latency_ms', 0) for r in records if isinstance(r.get('latency_ms'), (int, float))]
    avg_latency = sum(latencies) / len(latencies) if latencies else 0
    
    hits = sum(1 for r in records if r.get('cache_hit', False))
    hit_rate = hits / total if total > 0 else 0
    
    total_tokens = sum(r.get('prompt_tokens', 0) for r in records)
    avg_prompt_tokens = total_tokens / total if total > 0 else 0
    
    return FittedParams(
        total_requests=total,
        unique_requests=unique,
        repeat_ratio=repeat_ratio,
        semantic_cluster_ratio=semantic_ratio,
        estimated_zipf_alpha=alpha,
        conversation_ratio=conversation_ratio,
        avg_latency_ms=avg_latency,
        cache_hit_rate=hit_rate,
        total_tokens=total_tokens,
        avg_prompt_tokens=avg_prompt_tokens,
    )


def estimate_achievable(params: FittedParams) -> float:
    base = params.repeat_ratio
    semantic_bonus = params.semantic_cluster_ratio * (1 - params.repeat_ratio) * 0.5
    concentration_bonus = max(0, params.estimated_zipf_alpha - 1.0) * 0.1
    return min(0.98, base + semantic_bonus + concentration_bonus)


def print_report(params: FittedParams):
    print("\n" + "=" * 60)
    print("Shadow Log Analysis Report")
    print("=" * 60)
    
    print(f"\nTotal requests: {params.total_requests:,}")
    print(f"Unique requests: {params.unique_requests:,}")
    print(f"Total tokens: {params.total_tokens:,}")
    print(f"Avg prompt tokens: {params.avg_prompt_tokens:.1f}")
    
    print("\n--- Fitted Parameters ---")
    print(f"repeat_ratio:           {params.repeat_ratio * 100:.1f}%")
    print(f"semantic_cluster_ratio: {params.semantic_cluster_ratio * 100:.1f}%")
    print(f"conversation_ratio:     {params.conversation_ratio * 100:.1f}%")
    print(f"estimated_zipf_alpha:   {params.estimated_zipf_alpha:.2f}")
    
    print("\n--- Performance ---")
    print(f"avg_latency_ms:  {params.avg_latency_ms:.1f}ms")
    print(f"cache_hit_rate:  {params.cache_hit_rate * 100:.1f}%")
    
    achievable = estimate_achievable(params)
    print(f"\n--- Prediction ---")
    print(f"Estimated achievable hit rate: {achievable * 100:.1f}%")
    
    if achievable < 0.9:
        print("\n⚠️  Warning: Current trace pattern cannot reach 98% target.")
        print("   Consider: reducing unique_queries, increasing repeat_ratio")
        gap = 0.98 - achievable
        print(f"   Gap to target: {gap * 100:.1f} percentage points")
    elif achievable < 0.95:
        print("\nℹ️  Note: 98% target requires additional optimization.")
    else:
        print("\n✅ 98% target is achievable with optimal configuration.")
    
    print("\n--- Recommended LoadPattern ---")
    print("LoadPattern {")
    print(f"    unique_queries: {params.unique_requests},")
    print(f"    zipf_alpha: {params.estimated_zipf_alpha:.2f},")
    print(f"    repeat_ratio: {params.repeat_ratio:.2f},")
    print(f"    semantic_cluster_ratio: {params.semantic_cluster_ratio:.2f},")
    print(f"    conversation_ratio: {params.conversation_ratio:.2f},")
    print("    ..Default::default()")
    print("}")


def main():
    if len(sys.argv) < 2:
        print("Usage: python analyze_trace.py <trace.jsonl>")
        print("\nThe trace file should be a JSON Lines file from shadow logs.")
        print("Each line should be a JSON object with fields:")
        print("  - timestamp_ms: Unix timestamp")
        print("  - request_hash: SHA256 hash (first 16 chars)")
        print("  - content_length: Request body length")
        print("  - semantic_cluster: Semantic cluster bucket (0-99)")
        print("  - conversation_id: Optional conversation ID")
        print("  - model: Model name")
        print("  - prompt_tokens: Number of prompt tokens")
        print("  - latency_ms: Request latency")
        print("  - cache_hit: Boolean for cache status")
        sys.exit(1)
    
    path = sys.argv[1]
    print(f"Loading shadow log from: {path}")
    
    records = load_trace(path)
    print(f"Loaded {len(records)} trace records")
    
    if not records:
        print("\n⚠️  No valid trace records found in file.")
        print("   Make sure the file contains valid JSON lines.")
        sys.exit(1)
    
    params = analyze_trace(records)
    print_report(params)


if __name__ == "__main__":
    main()
