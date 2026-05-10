#!/usr/bin/env python3
"""
CrabCache HTTP Integration Test

This script tests the REAL running CrabCache gateway instance.
It does NOT simulate cache logic — it only generates load and measures responses.

Prerequisites:
    - CrabCache gateway running on target host (default: http://localhost:8080)
    - Valid API key configured
    - Prometheus metrics endpoint accessible (default: http://localhost:9090)

Usage:
    python cache_hit_rate_test.py --url http://localhost:8080 --requests 1000 --concurrency 50
"""

import asyncio
import aiohttp
import argparse
import json
import time
import random
from typing import List, Dict, Optional
from dataclasses import dataclass, field
from collections import defaultdict
from datetime import datetime


@dataclass
class TestConfig:
    gateway_url: str = "http://localhost:8080"
    metrics_url: str = "http://localhost:9090"
    api_key: str = "test-api-key"
    total_requests: int = 1000
    concurrency: int = 50
    pattern: str = "steady_state"  # "cold_start" | "steady_state" | "custom"
    seed: int = 42
    timeout_secs: float = 30.0


@dataclass
class RequestResult:
    request_id: str
    status_code: int
    response_time_ms: float
    cache_hit: bool
    cache_tier: Optional[str]
    error: Optional[str] = None


class TraceBasedGenerator:
    """Generates request payloads based on statistical load patterns.

    This replaces the old artificial base_questions with Zipf-distributed
    request patterns that approximate real-world behavior.
    """

    TOPICS = [
        "Explain quantum computing",
        "How does machine learning work",
        "What is blockchain",
        "Describe cloud architecture",
        "Explain neural networks",
        "What are data structures",
        "How to design APIs",
        "Explain distributed systems",
        "What is climate change",
        "How does encryption work",
    ]

    def __init__(self, pattern: str, seed: int):
        self.rng = random.Random(seed)
        self.pattern = pattern

        # Pattern parameters
        if pattern == "cold_start":
            self.unique_queries = 5000
            self.zipf_alpha = 0.8
            self.repeat_ratio = 0.05
        elif pattern == "steady_state":
            self.unique_queries = 500
            self.zipf_alpha = 1.5
            self.repeat_ratio = 0.6
        else:  # custom
            self.unique_queries = 1000
            self.zipf_alpha = 1.2
            self.repeat_ratio = 0.3

        self.templates = self._generate_templates()
        self.semantic_clusters = self._generate_clusters()

    def _generate_templates(self) -> List[str]:
        templates = []
        for i in range(self.unique_queries):
            topic = self.TOPICS[i % len(self.TOPICS)]
            templates.append(f"{topic} (query {i})")
        return templates

    def _generate_clusters(self) -> List[List[str]]:
        clusters = []
        for i in range(min(50, self.unique_queries // 20)):
            base = self.templates[i]
            clusters.append([
                f"What is {base}?",
                f"How does {base} work?",
                f"Explain {base} in simple terms",
                f"Can you describe {base}?",
                f"Tell me about {base}",
            ])
        return clusters

    def _zipf_sample(self) -> int:
        n = len(self.templates)
        alpha = self.zipf_alpha
        while True:
            rank = self.rng.random() * n
            prob = (1.0 / (rank + 1.0)) ** alpha
            if self.rng.random() < prob:
                return int(rank) % n

    def next_request(self, index: int) -> Dict:
        is_repeat = self.rng.random() < self.repeat_ratio
        is_semantic = self.rng.random() < 0.15

        if is_semantic and self.semantic_clusters:
            cluster = self.rng.choice(self.semantic_clusters)
            content = self.rng.choice(cluster)
        elif is_repeat:
            content = self.templates[self._zipf_sample()]
        else:
            content = f"{self.templates[self._zipf_sample()]} (variant {index})"

        return {
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": content}],
            "stream": False,
            "temperature": 0.7,
            "max_tokens": 100,
        }


class CacheHitRateTester:
    """Integration tester for the real CrabCache gateway."""

    def __init__(self, config: TestConfig):
        self.config = config
        self.generator = TraceBasedGenerator(config.pattern, config.seed)
        self.results: List[RequestResult] = []

    async def send_request(
        self,
        session: aiohttp.ClientSession,
        body: Dict,
        request_id: str,
    ) -> RequestResult:
        url = f"{self.config.gateway_url}/v1/chat/completions"
        headers = {
            "Authorization": f"Bearer {self.config.api_key}",
            "Content-Type": "application/json",
        }

        start = time.time()
        try:
            async with session.post(url, json=body, headers=headers) as resp:
                elapsed_ms = (time.time() - start) * 1000

                cache_hit = resp.headers.get("X-Cache-Status", "").upper() == "HIT"
                cache_tier = resp.headers.get("X-Cache-Tier")

                return RequestResult(
                    request_id=request_id,
                    status_code=resp.status,
                    response_time_ms=elapsed_ms,
                    cache_hit=cache_hit,
                    cache_tier=cache_tier,
                )
        except Exception as e:
            return RequestResult(
                request_id=request_id,
                status_code=0,
                response_time_ms=(time.time() - start) * 1000,
                cache_hit=False,
                cache_tier=None,
                error=str(e),
            )

    async def run(self):
        print(f"CrabCache Integration Test")
        print(f"  Gateway: {self.config.gateway_url}")
        print(f"  Pattern: {self.config.pattern}")
        print(f"  Requests: {self.config.total_requests}")
        print(f"  Concurrency: {self.config.concurrency}")
        print()

        timeout = aiohttp.ClientTimeout(total=self.config.timeout_secs)
        connector = aiohttp.TCPConnector(limit=self.config.concurrency)

        async with aiohttp.ClientSession(
            connector=connector, timeout=timeout
        ) as session:
            semaphore = asyncio.Semaphore(self.config.concurrency)

            async def bounded_request(i: int):
                async with semaphore:
                    body = self.generator.next_request(i)
                    result = await self.send_request(
                        session, body, f"req-{i:06d}"
                    )
                    self.results.append(result)

                    if (i + 1) % 100 == 0:
                        print(f"  Progress: {i + 1}/{self.config.total_requests}")

            await asyncio.gather(*[
                bounded_request(i) for i in range(self.config.total_requests)
            ])

        print("\nTest completed.")

    def analyze(self) -> Dict:
        if not self.results:
            return {}

        total = len(self.results)
        ok = [r for r in self.results if r.status_code == 200]
        hits = [r for r in ok if r.cache_hit]
        misses = [r for r in ok if not r.cache_hit]
        errors = [r for r in self.results if r.status_code != 200]

        times_ok = [r.response_time_ms for r in ok]
        times_hit = [r.response_time_ms for r in hits]
        times_miss = [r.response_time_ms for r in misses]

        def percentile(data: List[float], p: float) -> float:
            if not data:
                return 0.0
            s = sorted(data)
            k = (len(s) - 1) * p / 100.0
            f = int(k)
            c = min(f + 1, len(s) - 1)
            return s[f] + (k - f) * (s[c] - s[f])

        tier_counts = defaultdict(int)
        for r in ok:
            tier_counts[r.cache_tier or "unknown"] += 1

        return {
            "summary": {
                "total": total,
                "success": len(ok),
                "errors": len(errors),
                "success_rate_pct": len(ok) / total * 100 if total else 0,
            },
            "cache": {
                "hits": len(hits),
                "misses": len(misses),
                "hit_rate_pct": len(hits) / len(ok) * 100 if ok else 0,
            },
            "tiers": dict(tier_counts),
            "latency_ms": {
                "avg": sum(times_ok) / len(times_ok) if times_ok else 0,
                "p50": percentile(times_ok, 50),
                "p95": percentile(times_ok, 95),
                "p99": percentile(times_ok, 99),
            },
            "latency_comparison_ms": {
                "hit_avg": sum(times_hit) / len(times_hit) if times_hit else 0,
                "miss_avg": sum(times_miss) / len(times_miss) if times_miss else 0,
            },
        }

    def print_report(self, analysis: Dict):
        print("\n" + "=" * 60)
        print("Test Results")
        print("=" * 60)

        s = analysis["summary"]
        print(f"Total requests: {s['total']}")
        print(f"Success: {s['success']} ({s['success_rate_pct']:.1f}%)")
        print(f"Errors: {s['errors']}")

        c = analysis["cache"]
        print(f"\nCache hits: {c['hits']}")
        print(f"Cache misses: {c['misses']}")
        print(f"Hit rate: {c['hit_rate_pct']:.2f}%")

        print(f"\nLatency (ms):")
        for k, v in analysis["latency_ms"].items():
            print(f"  {k}: {v:.2f}")

        if analysis["latency_comparison_ms"]["hit_avg"] > 0:
            hit = analysis["latency_comparison_ms"]["hit_avg"]
            miss = analysis["latency_comparison_ms"]["miss_avg"]
            improvement = (miss - hit) / miss * 100 if miss > 0 else 0
            print(f"\nHit avg: {hit:.2f}ms")
            print(f"Miss avg: {miss:.2f}ms")
            print(f"Improvement: {improvement:.1f}%")

        print("=" * 60)


def main():
    parser = argparse.ArgumentParser(description="CrabCache integration test")
    parser.add_argument("--url", default="http://localhost:8080", help="Gateway URL")
    parser.add_argument("--api-key", default="test-api-key", help="API key")
    parser.add_argument("--requests", type=int, default=1000, help="Total requests")
    parser.add_argument("--concurrency", type=int, default=50, help="Concurrent requests")
    parser.add_argument("--pattern", default="steady_state", choices=["cold_start", "steady_state", "custom"])
    parser.add_argument("--seed", type=int, default=42, help="Random seed")
    parser.add_argument("--timeout", type=float, default=30.0, help="Request timeout")
    args = parser.parse_args()

    config = TestConfig(
        gateway_url=args.url,
        api_key=args.api_key,
        total_requests=args.requests,
        concurrency=args.concurrency,
        pattern=args.pattern,
        seed=args.seed,
        timeout_secs=args.timeout,
    )

    tester = CacheHitRateTester(config)
    asyncio.run(tester.run())

    analysis = tester.analyze()
    if analysis:
        tester.print_report(analysis)


if __name__ == "__main__":
    main()
