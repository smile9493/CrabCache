#!/usr/bin/env python3
"""
CrabCache 缓存模拟测试脚本

由于当前环境可能没有有效的上游 API key，此脚本将：
1. 模拟缓存命中/未命中场景
2. 计算理论缓存命中率
3. 生成性能分析报告
"""

import random
import time
import json
import hashlib
from typing import List, Dict, Tuple
from dataclasses import dataclass
from collections import defaultdict
import matplotlib.pyplot as plt
import pandas as pd
import numpy as np
from datetime import datetime


@dataclass
class SimulatedRequest:
    request_id: str
    prompt: str
    prompt_hash: str
    is_repeat: bool
    cache_hit: bool
    cache_tier: str
    response_time_ms: float
    timestamp: float


class CacheSimulator:
    """缓存模拟器"""
    
    def __init__(self):
        self.l0_cache = {}
        self.l1_cache = {}
        self.l2_semantic_cache = {}
        
        self.l0_max_capacity = 10000
        self.l0_ttl_secs = 3600
        self.l1_ttl_secs = 3600
        self.l2_ttl_secs = 86400
        
        self.l0_hit_count = 0
        self.l1_hit_count = 0
        self.l2_hit_count = 0
        self.miss_count = 0
        
    def _hash_prompt(self, prompt: str) -> str:
        """生成提示词哈希"""
        return hashlib.sha256(prompt.encode()).hexdigest()
    
    def _check_semantic_similarity(self, prompt: str) -> Tuple[bool, str]:
        """检查语义相似度（简化版）"""
        prompt_lower = prompt.lower().strip()
        
        for cached_prompt, cached_hash in self.l2_semantic_cache.items():
            cached_lower = cached_prompt.lower().strip()
            
            words1 = set(prompt_lower.split())
            words2 = set(cached_lower.split())
            
            if words1 and words2:
                similarity = len(words1 & words2) / len(words1 | words2)
                
                if similarity >= 0.7:
                    return True, cached_hash
        
        return False, ""
    
    def query_cache(self, prompt: str) -> Tuple[bool, str]:
        """查询缓存"""
        prompt_hash = self._hash_prompt(prompt)
        
        if prompt_hash in self.l0_cache:
            self.l0_hit_count += 1
            return True, "L0"
        
        if prompt_hash in self.l1_cache:
            self.l1_hit_count += 1
            self.l0_cache[prompt_hash] = time.time()
            return True, "L1"
        
        is_similar, similar_hash = self._check_semantic_similarity(prompt)
        if is_similar:
            self.l2_hit_count += 1
            self.l0_cache[prompt_hash] = time.time()
            return True, "L2"
        
        self.miss_count += 1
        self.l0_cache[prompt_hash] = time.time()
        self.l1_cache[prompt_hash] = time.time()
        self.l2_semantic_cache[prompt] = prompt_hash
        
        if len(self.l0_cache) > self.l0_max_capacity:
            oldest_keys = sorted(self.l0_cache.keys(), key=lambda k: self.l0_cache[k])[:100]
            for key in oldest_keys:
                del self.l0_cache[key]
        
        return False, "miss"


class TestDataGenerator:
    """测试数据生成器"""
    
    def __init__(self):
        self.base_questions = [
            "What is the capital of France?",
            "Explain quantum computing in simple terms.",
            "How does machine learning work?",
            "What are the benefits of cloud computing?",
            "Describe the process of photosynthesis.",
            "What is the difference between AI and ML?",
            "How do cryptocurrencies work?",
            "Explain the theory of relativity.",
            "What is climate change?",
            "How does the internet work?",
            "What is the meaning of life?",
            "Explain blockchain technology.",
            "What is artificial intelligence?",
            "How do neural networks work?",
            "What is the Big Bang theory?",
            "Explain DNA replication.",
            "What is the greenhouse effect?",
            "How does encryption work?",
            "What is machine learning?",
            "Explain the water cycle.",
        ]
        
        self.semantic_variations = {
            "What is the capital of France?": [
                "What's the capital city of France?",
                "Can you tell me the capital of France?",
                "What city is the capital of France?",
                "Which city is France's capital?",
                "What is France's capital city?",
            ],
            "Explain quantum computing in simple terms.": [
                "Can you explain quantum computing simply?",
                "What is quantum computing in easy words?",
                "Describe quantum computing for beginners.",
                "How would you explain quantum computing simply?",
                "What's a simple explanation of quantum computing?",
            ],
            "How does machine learning work?": [
                "Can you explain how machine learning works?",
                "What is the working principle of machine learning?",
                "Describe the mechanism of machine learning.",
                "How does ML function?",
                "Explain the process of machine learning.",
            ],
        }
    
    def generate_prompt(self, index: int, repeat_ratio: float = 0.3) -> Tuple[str, bool]:
        """生成测试提示词"""
        if random.random() < repeat_ratio:
            base_question = random.choice(self.base_questions)
            
            if base_question in self.semantic_variations and random.random() < 0.5:
                prompt = random.choice(self.semantic_variations[base_question])
                return prompt, True
            else:
                return base_question, True
        else:
            base_question = random.choice(self.base_questions)
            prompt = f"{base_question} (variation {index})"
            return prompt, False


class CacheHitRateSimulator:
    """缓存命中率模拟测试器"""
    
    def __init__(self, total_requests: int = 1000, repeat_ratio: float = 0.3):
        self.total_requests = total_requests
        self.repeat_ratio = repeat_ratio
        self.cache_simulator = CacheSimulator()
        self.data_generator = TestDataGenerator()
        self.results: List[SimulatedRequest] = []
        
    def run_simulation(self):
        """运行模拟测试"""
        print(f"开始缓存命中率模拟测试...")
        print(f"总请求数: {self.total_requests}")
        print(f"重复请求比例: {self.repeat_ratio * 100}%")
        print()
        
        start_time = time.time()
        
        for i in range(self.total_requests):
            prompt, is_repeat = self.data_generator.generate_prompt(i, self.repeat_ratio)
            prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()
            
            cache_hit, cache_tier = self.cache_simulator.query_cache(prompt)
            
            if cache_hit:
                if cache_tier == "L0":
                    response_time_ms = random.uniform(0.5, 2.0)
                elif cache_tier == "L1":
                    response_time_ms = random.uniform(3.0, 8.0)
                else:
                    response_time_ms = random.uniform(10.0, 20.0)
            else:
                response_time_ms = random.uniform(50.0, 150.0)
            
            result = SimulatedRequest(
                request_id=f"req-{i:06d}",
                prompt=prompt,
                prompt_hash=prompt_hash,
                is_repeat=is_repeat,
                cache_hit=cache_hit,
                cache_tier=cache_tier,
                response_time_ms=response_time_ms,
                timestamp=time.time(),
            )
            
            self.results.append(result)
            
            if (i + 1) % 100 == 0:
                progress = (i + 1) / self.total_requests * 100
                print(f"\r进度: {progress:.1f}% ({i + 1}/{self.total_requests})", end="")
        
        elapsed_time = time.time() - start_time
        print(f"\n\n模拟测试完成！耗时: {elapsed_time:.2f}秒")
    
    def analyze_results(self) -> Dict:
        """分析模拟结果"""
        if not self.results:
            return {}
        
        total_requests = len(self.results)
        
        cache_hits = [r for r in self.results if r.cache_hit]
        cache_misses = [r for r in self.results if not r.cache_hit]
        
        cache_tier_stats = defaultdict(int)
        for r in self.results:
            cache_tier_stats[r.cache_tier] += 1
        
        response_times = [r.response_time_ms for r in self.results]
        hit_response_times = [r.response_time_ms for r in cache_hits]
        miss_response_times = [r.response_time_ms for r in cache_misses]
        
        repeat_requests = [r for r in self.results if r.is_repeat]
        repeat_hits = [r for r in repeat_requests if r.cache_hit]
        
        analysis = {
            "summary": {
                "total_requests": total_requests,
                "cache_hits": len(cache_hits),
                "cache_misses": len(cache_misses),
                "hit_rate": len(cache_hits) / total_requests * 100,
                "miss_rate": len(cache_misses) / total_requests * 100,
            },
            "cache_tier_distribution": dict(cache_tier_stats),
            "cache_tier_hit_rates": {
                "L0": self.cache_simulator.l0_hit_count,
                "L1": self.cache_simulator.l1_hit_count,
                "L2": self.cache_simulator.l2_hit_count,
            },
            "response_time": {
                "avg_ms": np.mean(response_times),
                "p50_ms": np.percentile(response_times, 50),
                "p95_ms": np.percentile(response_times, 95),
                "p99_ms": np.percentile(response_times, 99),
                "min_ms": min(response_times),
                "max_ms": max(response_times),
            },
            "hit_vs_miss_latency": {
                "hit_avg_ms": np.mean(hit_response_times) if hit_response_times else 0,
                "miss_avg_ms": np.mean(miss_response_times) if miss_response_times else 0,
                "latency_improvement": (
                    (np.mean(miss_response_times) - np.mean(hit_response_times)) / np.mean(miss_response_times) * 100
                    if hit_response_times and miss_response_times else 0
                ),
            },
            "repeat_pattern": {
                "total_repeats": len(repeat_requests),
                "repeat_hits": len(repeat_hits),
                "repeat_hit_rate": len(repeat_hits) / len(repeat_requests) * 100 if repeat_requests else 0,
            },
        }
        
        return analysis
    
    def generate_report(self, analysis: Dict, output_file: str = "cache_simulation_report.md"):
        """生成模拟测试报告"""
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        
        report = f"""# CrabCache 缓存命中率模拟测试报告

**测试时间**: {timestamp}

**测试类型**: 模拟测试（无需真实上游 API）

## 测试配置

- **总请求数**: {self.total_requests}
- **重复请求比例**: {self.repeat_ratio * 100}%

## 测试摘要

| 指标 | 数值 |
|------|------|
| 总请求数 | {analysis['summary']['total_requests']} |
| 缓存命中数 | {analysis['summary']['cache_hits']} |
| 缓存未命中数 | {analysis['summary']['cache_misses']} |
| **缓存命中率** | **{analysis['summary']['hit_rate']:.2f}%** |
| 缓存未命中率 | {analysis['summary']['miss_rate']:.2f}% |

## 缓存层级分布

| 缓存层级 | 命中次数 | 占比 |
|---------|---------|------|
"""
        
        total = sum(analysis['cache_tier_distribution'].values())
        for tier, count in sorted(analysis['cache_tier_distribution'].items()):
            percentage = count / total * 100 if total > 0 else 0
            report += f"| {tier} | {count} | {percentage:.2f}% |\n"
        
        report += f"""
## 各层级命中统计

| 缓存层级 | 命中次数 |
|---------|---------|
| L0 (内存缓存) | {analysis['cache_tier_hit_rates']['L0']} |
| L1 (Redis) | {analysis['cache_tier_hit_rates']['L1']} |
| L2 (语义缓存) | {analysis['cache_tier_hit_rates']['L2']} |

## 响应时间分析

| 指标 | 数值 (ms) |
|------|-----------|
| 平均响应时间 | {analysis['response_time']['avg_ms']:.2f} |
| P50 响应时间 | {analysis['response_time']['p50_ms']:.2f} |
| P95 响应时间 | {analysis['response_time']['p95_ms']:.2f} |
| P99 响应时间 | {analysis['response_time']['p99_ms']:.2f} |
| 最小响应时间 | {analysis['response_time']['min_ms']:.2f} |
| 最大响应时间 | {analysis['response_time']['max_ms']:.2f} |

## 命中 vs 未命中延迟对比

| 指标 | 数值 (ms) |
|------|-----------|
| 命中平均延迟 | {analysis['hit_vs_miss_latency']['hit_avg_ms']:.2f} |
| 未命中平均延迟 | {analysis['hit_vs_miss_latency']['miss_avg_ms']:.2f} |
| **延迟改善** | **{analysis['hit_vs_miss_latency']['latency_improvement']:.2f}%** |

## 重复请求模式分析

| 指标 | 数值 |
|------|------|
| 重复请求总数 | {analysis['repeat_pattern']['total_repeats']} |
| 重复请求命中数 | {analysis['repeat_pattern']['repeat_hits']} |
| 重复请求命中率 | {analysis['repeat_pattern']['repeat_hit_rate']:.2f}% |

## 性能评估

"""
        
        hit_rate = analysis['summary']['hit_rate']
        latency_improvement = analysis['hit_vs_miss_latency']['latency_improvement']
        
        if hit_rate >= 80:
            report += "✅ **缓存命中率优秀** (≥80%)\n"
        elif hit_rate >= 60:
            report += "⚠️ **缓存命中率良好** (60-80%)\n"
        else:
            report += "❌ **缓存命中率需优化** (<60%)\n"
        
        if latency_improvement >= 50:
            report += "✅ **延迟改善显著** (≥50%)\n"
        elif latency_improvement >= 30:
            report += "⚠️ **延迟改善一般** (30-50%)\n"
        else:
            report += "❌ **延迟改善不明显** (<30%)\n"
        
        report += """
## 优化建议

### 1. 提高缓存命中率

根据模拟结果，建议采取以下措施：

- **增加 L0 内存缓存容量**: 当前配置为 10,000，可根据实际内存情况调整
- **优化 TTL 配置**: 根据数据访问模式调整各层级 TTL
- **语义缓存调优**: 调整相似度阈值以平衡准确率和召回率

### 2. 降低延迟

- **L0 缓存优化**: 使用更高效的数据结构（如 DashMap）
- **L1 连接池**: 优化 Redis 连接池配置
- **L2 向量搜索**: 使用 ANN 算法加速相似度搜索

### 3. 监控指标

建议监控以下关键指标：

```promql
# 缓存命中率
sum(rate(gateway_cache_requests_total{result="hit"}[5m])) 
  / sum(rate(gateway_cache_requests_total[5m]))

# 各层级延迟
histogram_quantile(0.99, 
  sum(rate(gateway_upstream_latency_seconds_bucket[5m])) by (le)
)
```

## 测试数据可视化

测试过程中生成了以下可视化图表：
- `cache_simulation_visualization.png`: 缓存性能可视化

---
*报告由 CrabCache 缓存模拟测试工具自动生成*
"""
        
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write(report)
        
        print(f"模拟测试报告已生成: {output_file}")
    
    def visualize_results(self, analysis: Dict):
        """可视化模拟结果"""
        fig, axes = plt.subplots(2, 2, figsize=(15, 12))
        
        timestamps = [r.timestamp for r in self.results]
        cache_hits = [1 if r.cache_hit else 0 for r in self.results]
        
        window_size = max(1, len(self.results) // 20)
        hit_rate_timeline = pd.Series(cache_hits).rolling(window=window_size).mean() * 100
        
        axes[0, 0].plot(range(len(hit_rate_timeline)), hit_rate_timeline, linewidth=2, color='blue')
        axes[0, 0].set_title('Cache Hit Rate Timeline', fontsize=14, fontweight='bold')
        axes[0, 0].set_xlabel('Request Number')
        axes[0, 0].set_ylabel('Hit Rate (%)')
        axes[0, 0].grid(True, alpha=0.3)
        axes[0, 0].axhline(y=30, color='red', linestyle='--', label='Expected (30%)')
        axes[0, 0].legend()
        
        response_times = [r.response_time_ms for r in self.results]
        
        axes[0, 1].hist(response_times, bins=50, color='skyblue', edgecolor='black', alpha=0.7)
        axes[0, 1].axvline(np.mean(response_times), color='red', linestyle='--', 
                          label=f'Mean: {np.mean(response_times):.2f}ms')
        axes[0, 1].set_title('Response Time Distribution', fontsize=14, fontweight='bold')
        axes[0, 1].set_xlabel('Response Time (ms)')
        axes[0, 1].set_ylabel('Frequency')
        axes[0, 1].legend()
        axes[0, 1].grid(True, alpha=0.3)
        
        tier_distribution = analysis['cache_tier_distribution']
        if tier_distribution:
            labels = list(tier_distribution.keys())
            sizes = list(tier_distribution.values())
            colors = ['#ff9999', '#66b3ff', '#99ff99', '#ffcc99']
            
            axes[1, 0].pie(sizes, labels=labels, colors=colors[:len(labels)], 
                          autopct='%1.1f%%', startangle=90)
            axes[1, 0].set_title('Cache Tier Distribution', fontsize=14, fontweight='bold')
        
        hit_times = [r.response_time_ms for r in self.results if r.cache_hit]
        miss_times = [r.response_time_ms for r in self.results if not r.cache_hit]
        
        if hit_times and miss_times:
            axes[1, 1].boxplot([hit_times, miss_times], labels=['Cache Hit', 'Cache Miss'])
            axes[1, 1].set_title('Hit vs Miss Response Time', fontsize=14, fontweight='bold')
            axes[1, 1].set_ylabel('Response Time (ms)')
            axes[1, 1].grid(True, alpha=0.3)
        
        plt.tight_layout()
        plt.savefig('cache_simulation_visualization.png', dpi=300, bbox_inches='tight')
        print("可视化图表已生成: cache_simulation_visualization.png")
        
        plt.close()


def main():
    simulator = CacheHitRateSimulator(
        total_requests=1000,
        repeat_ratio=0.3,
    )
    
    simulator.run_simulation()
    
    analysis = simulator.analyze_results()
    
    if analysis:
        print("\n" + "="*60)
        print("模拟测试结果摘要")
        print("="*60)
        print(f"总请求数: {analysis['summary']['total_requests']}")
        print(f"缓存命中数: {analysis['summary']['cache_hits']}")
        print(f"缓存命中率: {analysis['summary']['hit_rate']:.2f}%")
        print(f"平均响应时间: {analysis['response_time']['avg_ms']:.2f}ms")
        print(f"P95 响应时间: {analysis['response_time']['p95_ms']:.2f}ms")
        print(f"延迟改善: {analysis['hit_vs_miss_latency']['latency_improvement']:.2f}%")
        print("="*60)
        
        simulator.generate_report(analysis)
        simulator.visualize_results(analysis)


if __name__ == "__main__":
    main()
