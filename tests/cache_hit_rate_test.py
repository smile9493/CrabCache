#!/usr/bin/env python3
"""
CrabCache 缓存命中率模拟测试脚本

测试场景：
1. L0 (Moka) 内存缓存命中率
2. L1 (Redis) 分布式缓存命中率
3. L2 (Qdrant) 语义缓存命中率
4. 综合缓存命中率分析

测试策略：
- 重复请求模式：模拟用户重复提问
- 语义相似请求：测试语义缓存效果
- 冷启动场景：测试缓存预热效果
- 高并发场景：测试缓存并发性能
"""

import asyncio
import aiohttp
import json
import time
import random
import hashlib
from typing import List, Dict, Tuple
from dataclasses import dataclass
from collections import defaultdict
import matplotlib.pyplot as plt
import pandas as pd
import numpy as np
from datetime import datetime


@dataclass
class TestConfig:
    gateway_url: str = "http://localhost:8080"
    metrics_url: str = "http://localhost:9090"
    api_key: str = "test-api-key"
    total_requests: int = 1000
    concurrent_requests: int = 10
    repeat_ratio: float = 0.3
    semantic_similarity_threshold: float = 0.95
    test_duration_secs: int = 60


@dataclass
class RequestResult:
    request_id: str
    status_code: int
    response_time_ms: float
    cache_hit: bool
    cache_tier: str
    model: str
    prompt_tokens: int
    completion_tokens: int
    timestamp: float


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
        """
        生成测试提示词
        
        Args:
            index: 请求索引
            repeat_ratio: 重复请求比例
            
        Returns:
            (prompt, is_repeat): 提示词和是否为重复请求
        """
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
    
    def generate_request_body(self, prompt: str, model: str = "deepseek-v4-pro") -> Dict:
        """生成请求体"""
        return {
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": False,
            "temperature": 0.7,
            "max_tokens": 100,
        }


class CacheHitRateTester:
    """缓存命中率测试器"""
    
    def __init__(self, config: TestConfig):
        self.config = config
        self.data_generator = TestDataGenerator()
        self.results: List[RequestResult] = []
        self.request_times: List[float] = []
        
    async def send_request(
        self, 
        session: aiohttp.ClientSession, 
        prompt: str, 
        request_id: str
    ) -> RequestResult:
        """发送单个请求"""
        url = f"{self.config.gateway_url}/v1/chat/completions"
        headers = {
            "Authorization": f"Bearer {self.config.api_key}",
            "Content-Type": "application/json",
        }
        
        body = self.data_generator.generate_request_body(prompt)
        
        start_time = time.time()
        
        try:
            async with session.post(url, json=body, headers=headers) as response:
                response_time_ms = (time.time() - start_time) * 1000
                
                if response.status == 200:
                    data = await response.json()
                    
                    cache_hit = response.headers.get("X-Cache-Hit", "false").lower() == "true"
                    cache_tier = response.headers.get("X-Cache-Tier", "miss")
                    
                    usage = data.get("usage", {})
                    
                    return RequestResult(
                        request_id=request_id,
                        status_code=response.status,
                        response_time_ms=response_time_ms,
                        cache_hit=cache_hit,
                        cache_tier=cache_tier,
                        model=data.get("model", "unknown"),
                        prompt_tokens=usage.get("prompt_tokens", 0),
                        completion_tokens=usage.get("completion_tokens", 0),
                        timestamp=time.time(),
                    )
                else:
                    return RequestResult(
                        request_id=request_id,
                        status_code=response.status,
                        response_time_ms=response_time_ms,
                        cache_hit=False,
                        cache_tier="error",
                        model="unknown",
                        prompt_tokens=0,
                        completion_tokens=0,
                        timestamp=time.time(),
                    )
        except Exception as e:
            response_time_ms = (time.time() - start_time) * 1000
            return RequestResult(
                request_id=request_id,
                status_code=0,
                response_time_ms=response_time_ms,
                cache_hit=False,
                cache_tier="error",
                model="unknown",
                prompt_tokens=0,
                completion_tokens=0,
                timestamp=time.time(),
            )
    
    async def run_concurrent_test(self):
        """运行并发测试"""
        print(f"开始缓存命中率测试...")
        print(f"总请求数: {self.config.total_requests}")
        print(f"并发数: {self.config.concurrent_requests}")
        print(f"重复请求比例: {self.config.repeat_ratio * 100}%")
        print()
        
        connector = aiohttp.TCPConnector(limit=self.config.concurrent_requests)
        timeout = aiohttp.ClientTimeout(total=30)
        
        async with aiohttp.ClientSession(connector=connector, timeout=timeout) as session:
            tasks = []
            
            for i in range(self.config.total_requests):
                prompt, is_repeat = self.data_generator.generate_prompt(
                    i, 
                    self.config.repeat_ratio
                )
                
                request_id = f"req-{i:06d}"
                task = self.send_request(session, prompt, request_id)
                tasks.append(task)
                
                if len(tasks) >= self.config.concurrent_requests:
                    results = await asyncio.gather(*tasks)
                    self.results.extend(results)
                    tasks = []
                    
                    progress = (i + 1) / self.config.total_requests * 100
                    print(f"\r进度: {progress:.1f}% ({i + 1}/{self.config.total_requests})", end="")
            
            if tasks:
                results = await asyncio.gather(*tasks)
                self.results.extend(results)
        
        print("\n\n测试完成！")
    
    def analyze_results(self) -> Dict:
        """分析测试结果"""
        if not self.results:
            return {}
        
        total_requests = len(self.results)
        successful_requests = [r for r in self.results if r.status_code == 200]
        
        cache_hits = [r for r in successful_requests if r.cache_hit]
        cache_misses = [r for r in successful_requests if not r.cache_hit]
        
        cache_tier_stats = defaultdict(int)
        for r in successful_requests:
            cache_tier_stats[r.cache_tier] += 1
        
        response_times = [r.response_time_ms for r in successful_requests]
        
        hit_response_times = [r.response_time_ms for r in cache_hits]
        miss_response_times = [r.response_time_ms for r in cache_misses]
        
        total_tokens = sum(r.prompt_tokens + r.completion_tokens for r in successful_requests)
        
        analysis = {
            "summary": {
                "total_requests": total_requests,
                "successful_requests": len(successful_requests),
                "failed_requests": total_requests - len(successful_requests),
                "success_rate": len(successful_requests) / total_requests * 100,
            },
            "cache_performance": {
                "total_hits": len(cache_hits),
                "total_misses": len(cache_misses),
                "hit_rate": len(cache_hits) / len(successful_requests) * 100 if successful_requests else 0,
                "miss_rate": len(cache_misses) / len(successful_requests) * 100 if successful_requests else 0,
            },
            "cache_tier_distribution": dict(cache_tier_stats),
            "response_time": {
                "avg_ms": np.mean(response_times) if response_times else 0,
                "p50_ms": np.percentile(response_times, 50) if response_times else 0,
                "p95_ms": np.percentile(response_times, 95) if response_times else 0,
                "p99_ms": np.percentile(response_times, 99) if response_times else 0,
                "min_ms": min(response_times) if response_times else 0,
                "max_ms": max(response_times) if response_times else 0,
            },
            "hit_vs_miss_latency": {
                "hit_avg_ms": np.mean(hit_response_times) if hit_response_times else 0,
                "miss_avg_ms": np.mean(miss_response_times) if miss_response_times else 0,
                "latency_improvement": (
                    (np.mean(miss_response_times) - np.mean(hit_response_times)) / np.mean(miss_response_times) * 100
                    if hit_response_times and miss_response_times else 0
                ),
            },
            "token_usage": {
                "total_tokens": total_tokens,
                "avg_tokens_per_request": total_tokens / len(successful_requests) if successful_requests else 0,
            },
        }
        
        return analysis
    
    def generate_report(self, analysis: Dict, output_file: str = "cache_test_report.md"):
        """生成测试报告"""
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        
        report = f"""# CrabCache 缓存命中率测试报告

**测试时间**: {timestamp}

## 测试配置

- **网关地址**: {self.config.gateway_url}
- **总请求数**: {self.config.total_requests}
- **并发数**: {self.config.concurrent_requests}
- **重复请求比例**: {self.config.repeat_ratio * 100}%

## 测试摘要

| 指标 | 数值 |
|------|------|
| 总请求数 | {analysis['summary']['total_requests']} |
| 成功请求 | {analysis['summary']['successful_requests']} |
| 失败请求 | {analysis['summary']['failed_requests']} |
| 成功率 | {analysis['summary']['success_rate']:.2f}% |

## 缓存性能

| 指标 | 数值 |
|------|------|
| 缓存命中数 | {analysis['cache_performance']['total_hits']} |
| 缓存未命中数 | {analysis['cache_performance']['total_misses']} |
| **缓存命中率** | **{analysis['cache_performance']['hit_rate']:.2f}%** |
| 缓存未命中率 | {analysis['cache_performance']['miss_rate']:.2f}% |

## 缓存层级分布

| 缓存层级 | 命中次数 | 占比 |
|---------|---------|------|
"""
        
        total = sum(analysis['cache_tier_distribution'].values())
        for tier, count in sorted(analysis['cache_tier_distribution'].items()):
            percentage = count / total * 100 if total > 0 else 0
            report += f"| {tier} | {count} | {percentage:.2f}% |\n"
        
        report += f"""
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

## Token 使用统计

| 指标 | 数值 |
|------|------|
| 总 Token 数 | {analysis['token_usage']['total_tokens']} |
| 平均每请求 Token 数 | {analysis['token_usage']['avg_tokens_per_request']:.2f} |

## 性能评估

"""
        
        hit_rate = analysis['cache_performance']['hit_rate']
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

"""
        
        if hit_rate < 60:
            report += """1. **提高缓存命中率**:
   - 增加 L0 内存缓存容量
   - 调整 L1 Redis TTL 配置
   - 优化 L2 语义缓存相似度阈值
   - 分析请求模式，识别热点查询

"""
        
        if latency_improvement < 30:
            report += """2. **降低延迟**:
   - 检查网络连接质量
   - 优化上游 API 响应时间
   - 调整并发连接数配置
   - 启用连接池复用

"""
        
        if analysis['summary']['failed_requests'] > 0:
            report += """3. **提高可靠性**:
   - 检查错误日志，识别失败原因
   - 增加重试机制
   - 优化超时配置
   - 添加熔断器保护

"""
        
        report += """## 测试数据可视化

测试过程中生成了以下可视化图表：
- `cache_hit_rate_timeline.png`: 缓存命中率时间线
- `response_time_distribution.png`: 响应时间分布
- `cache_tier_distribution.png`: 缓存层级分布

---
*报告由 CrabCache 缓存测试工具自动生成*
"""
        
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write(report)
        
        print(f"测试报告已生成: {output_file}")
    
    def visualize_results(self, analysis: Dict):
        """可视化测试结果"""
        if not self.results:
            return
        
        fig, axes = plt.subplots(2, 2, figsize=(15, 12))
        
        timestamps = [r.timestamp for r in self.results]
        cache_hits = [1 if r.cache_hit else 0 for r in self.results]
        
        window_size = max(1, len(self.results) // 20)
        hit_rate_timeline = pd.Series(cache_hits).rolling(window=window_size).mean() * 100
        
        axes[0, 0].plot(timestamps, hit_rate_timeline, linewidth=2)
        axes[0, 0].set_title('Cache Hit Rate Timeline', fontsize=14, fontweight='bold')
        axes[0, 0].set_xlabel('Time')
        axes[0, 0].set_ylabel('Hit Rate (%)')
        axes[0, 0].grid(True, alpha=0.3)
        
        response_times = [r.response_time_ms for r in self.results if r.status_code == 200]
        
        axes[0, 1].hist(response_times, bins=50, color='skyblue', edgecolor='black', alpha=0.7)
        axes[0, 1].axvline(np.mean(response_times), color='red', linestyle='--', label=f'Mean: {np.mean(response_times):.2f}ms')
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
            
            axes[1, 0].pie(sizes, labels=labels, colors=colors[:len(labels)], autopct='%1.1f%%', startangle=90)
            axes[1, 0].set_title('Cache Tier Distribution', fontsize=14, fontweight='bold')
        
        hit_times = [r.response_time_ms for r in self.results if r.cache_hit and r.status_code == 200]
        miss_times = [r.response_time_ms for r in self.results if not r.cache_hit and r.status_code == 200]
        
        if hit_times and miss_times:
            axes[1, 1].boxplot([hit_times, miss_times], labels=['Cache Hit', 'Cache Miss'])
            axes[1, 1].set_title('Hit vs Miss Response Time', fontsize=14, fontweight='bold')
            axes[1, 1].set_ylabel('Response Time (ms)')
            axes[1, 1].grid(True, alpha=0.3)
        
        plt.tight_layout()
        plt.savefig('cache_test_visualization.png', dpi=300, bbox_inches='tight')
        print("可视化图表已生成: cache_test_visualization.png")
        
        plt.close()


async def main():
    config = TestConfig(
        gateway_url="http://localhost:8080",
        metrics_url="http://localhost:9090",
        api_key="test-api-key",
        total_requests=100,
        concurrent_requests=10,
        repeat_ratio=0.4,
    )
    
    tester = CacheHitRateTester(config)
    
    await tester.run_concurrent_test()
    
    analysis = tester.analyze_results()
    
    if analysis:
        print("\n" + "="*60)
        print("测试结果摘要")
        print("="*60)
        print(f"总请求数: {analysis['summary']['total_requests']}")
        print(f"成功率: {analysis['summary']['success_rate']:.2f}%")
        print(f"缓存命中率: {analysis['cache_performance']['hit_rate']:.2f}%")
        print(f"平均响应时间: {analysis['response_time']['avg_ms']:.2f}ms")
        print(f"P95 响应时间: {analysis['response_time']['p95_ms']:.2f}ms")
        print(f"延迟改善: {analysis['hit_vs_miss_latency']['latency_improvement']:.2f}%")
        print("="*60)
        
        tester.generate_report(analysis)
        tester.visualize_results(analysis)


if __name__ == "__main__":
    asyncio.run(main())
