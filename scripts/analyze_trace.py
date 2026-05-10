#!/usr/bin/env python3
"""
Trace Analyzer - Analyze collected traces and fit simulation parameters.

Usage:
    python analyze_trace.py trace.jsonl
    
The trace file should contain JSON lines from the gateway logs, each with:
    - request_hash: SHA256 hash of the request body
    - content_length: Length of the request body in bytes
    - latency_ms: Request latency in milliseconds
    - cache_hit: Boolean indicating if cache was hit
    - conversation_id: Optional conversation identifier
    - total_tokens: Total tokens used in the request
"""

import json
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from typing import List, Dict, Optional


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


def load_trace(path: str) -> List[dict]:
    records = []
    with open(path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
                if record.get('message') == 'Request completed':
                    records.append(record)
            except json.JSONDecodeError:
                continue
    return records


def analyze_trace(records: List[dict]) -> FittedParams:
    if not records:
        return FittedParams(0, 0, 0, 0, 1.0, 0, 0, 0, 0)
    
    total = len(records)
    
    hashes = [r.get('request_hash', r.get('fields', {}).get('request_hash', 'unknown')) for r in records]
    unique_hashes = set(hashes)
    unique = len(unique_hashes)
    
    repeat_ratio = 1.0 - (unique / total) if total > 0 else 0
    
    freq = Counter(hashes)
    counts = sorted(freq.values(), reverse=True)
    
    top_10 = counts[:max(1, len(counts) // 10)]
    top_sum = sum(top_10)
    total_sum = sum(counts)
    concentration = top_sum / total_sum if total_sum > 0 else 0
    
    if concentration > 0.8:
        zipf_alpha = 1.8
    elif concentration > 0.6:
        zipf_alpha = 1.5
    elif concentration > 0.4:
        zipf_alpha = 1.2
    else:
        zipf_alpha = 1.0
    
    length_clusters = defaultdict(int)
    for r in records:
        content_length = r.get('content_length', r.get('fields', {}).get('content_length', 0))
        bucket = content_length // 100
        length_clusters[bucket] += 1
    
    clustered = sum(c for c in length_clusters.values() if c > 1)
    semantic_ratio = clustered / total if total > 0 else 0
    
    with_conv = sum(1 for r in records 
                    if r.get('conversation_id') or r.get('fields', {}).get('conversation_id'))
    conversation_ratio = with_conv / total if total > 0 else 0
    
    latencies = []
    for r in records:
        latency = r.get('latency_ms', r.get('fields', {}).get('latency_ms', 0))
        if isinstance(latency, (int, float)):
            latencies.append(latency)
    avg_latency = sum(latencies) / len(latencies) if latencies else 0
    
    hits = sum(1 for r in records 
               if r.get('cache_hit') or r.get('fields', {}).get('cache_hit'))
    hit_rate = hits / total if total > 0 else 0
    
    total_tokens = sum(
        r.get('total_tokens', r.get('fields', {}).get('total_tokens', 0))
        for r in records
    )
    
    return FittedParams(
        total_requests=total,
        unique_requests=unique,
        repeat_ratio=repeat_ratio,
        semantic_cluster_ratio=semantic_ratio,
        estimated_zipf_alpha=zipf_alpha,
        conversation_ratio=conversation_ratio,
        avg_latency_ms=avg_latency,
        cache_hit_rate=hit_rate,
        total_tokens=total_tokens,
    )


def estimate_achievable(params: FittedParams) -> float:
    base = params.repeat_ratio
    semantic_bonus = params.semantic_cluster_ratio * (1 - params.repeat_ratio) * 0.5
    concentration_bonus = max(0, params.estimated_zipf_alpha - 1.0) * 0.1
    return min(0.98, base + semantic_bonus + concentration_bonus)


def print_report(params: FittedParams):
    print("\n" + "=" * 50)
    print("Trace Analysis Report")
    print("=" * 50)
    
    print(f"\nTotal requests: {params.total_requests}")
    print(f"Unique requests: {params.unique_requests}")
    print(f"Total tokens: {params.total_tokens}")
    
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
    print(f"    zipf_alpha: {params.estimated_zipf_alpha:.1f},")
    print(f"    repeat_ratio: {params.repeat_ratio:.2f},")
    print(f"    semantic_cluster_ratio: {params.semantic_cluster_ratio:.2f},")
    print(f"    conversation_ratio: {params.conversation_ratio:.2f},")
    print("    ..Default::default()")
    print("}")


def main():
    if len(sys.argv) < 2:
        print("Usage: python analyze_trace.py <trace.jsonl>")
        print("\nThe trace file should be a JSON Lines file from gateway logs.")
        print("Each line should be a JSON object with fields:")
        print("  - request_hash: SHA256 hash of request body")
        print("  - content_length: Request body length")
        print("  - latency_ms: Request latency")
        print("  - cache_hit: Boolean for cache status")
        print("  - conversation_id: Optional conversation ID")
        print("  - total_tokens: Total tokens used")
        sys.exit(1)
    
    path = sys.argv[1]
    print(f"Loading trace from: {path}")
    
    records = load_trace(path)
    print(f"Loaded {len(records)} request records")
    
    if not records:
        print("\n⚠️  No valid request records found in trace file.")
        print("   Make sure the file contains JSON logs with 'Request completed' messages.")
        sys.exit(1)
    
    params = analyze_trace(records)
    print_report(params)


if __name__ == "__main__":
    main()
