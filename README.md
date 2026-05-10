# CrabCache - DeepSeek V4 高性能 Rust API 网关

<div align="center">

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/yourusername/crabcache)

**一个基于 Pingora 的高性能 API 网关，专为 DeepSeek V4 大语言模型设计**

[特性](#特性) • [架构](#架构) • [快速开始](#快速开始) • [配置](#配置) • [性能](#性能) • [测试](#测试)

</div>

---

## 📖 概述

CrabCache 是一个生产级的 Rust API 网关，通过三级缓存架构和会话亲和性路由，实现极致的成本优化和低延迟响应。基于 Cloudflare Pingora 框架构建，提供亚毫秒级延迟和无锁连接池。

### 🎯 核心目标

- **成本优化**: 通过多级缓存减少 95%+ 的 API 调用成本
- **低延迟**: P99 延迟 < 1ms（缓存命中场景）
- **高可用**: 支持多密钥管理、请求合并、优雅降级
- **可观测**: 完整的 Prometheus 指标和结构化日志

## ✨ 特性

### 🚀 高性能架构

- **基于 Pingora**: Cloudflare 开源的高性能网络框架
- **零拷贝优化**: 使用 `bytes::Bytes` 实现 O(1) 克隆
- **无锁连接池**: 两级连接池设计，90% 流量无锁处理
- **jemalloc**: 生产级内存分配器，优化高并发性能

### 🗄️ 三级缓存防御体系

| 层级 | 技术 | 延迟 | 命中率目标 |
|------|------|------|-----------|
| **L0** | Moka 内存缓存 | P99 < 100ns | 60-70% |
| **L1** | Redis 分布式缓存 | P99 < 5ms | 20-30% |
| **L2** | Qdrant 语义缓存 | P99 < 20ms | 5-10% |

### 🔄 智能路由

- **Ketama 一致性哈希**: 最大化 DeepSeek V4 前缀缓存命中率
- **会话亲和性**: 相同 conversation_id 路由到同一后端节点
- **动态权重**: 支持后端节点动态扩缩容

### 🛡️ 企业级特性

- **多密钥管理**: 支持多租户 API 密钥管理
- **请求合并**: 防止缓存击穿（Cache Stampede）
- **优雅降级**: 缓存失败时自动降级
- **流式响应**: SSE 流式响应优化

## 🏗️ 架构

```
┌─────────────────────────────────────────────────────────────┐
│                        CrabCache Gateway                     │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   L0 Cache  │  │   L1 Cache  │  │   L2 Cache  │        │
│  │    (Moka)   │  │   (Redis)   │  │  (Qdrant)   │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   Router    │  │  Coalescer  │  │   Metrics   │        │
│  │  (Ketama)   │  │ (Request)   │  │ (Prometheus)│        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │  DeepSeek V4    │
                    │     API         │
                    └─────────────────┘
```

## 🚦 快速开始

### 环境要求

- **Rust**: 2024 Edition (stable)
- **系统**: Linux 内核（生产环境必需）
- **依赖服务**: Redis, Qdrant (可选)

### 安装

```bash
# 克隆仓库
git clone https://github.com/yourusername/crabcache.git
cd crabcache

# 构建项目
cargo build --release

# 复制配置文件
cp config/gateway.example.toml config/gateway.toml
```

### 配置

编辑 `config/gateway.toml`:

```toml
listen_addr = "0.0.0.0:8080"
metrics_addr = "0.0.0.0:9090"
api_key = "your-deepseek-api-key"

[upstream]
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
deepseek_endpoints = ["api.deepseek.com:443"]

[cache]
l0_max_capacity = 10000
l0_ttl_secs = 3600
l1_redis_url = "redis://127.0.0.1:6379"

[semantic]
enabled = false
model_path = "models/all-MiniLM-L6-v2.onnx"
qdrant_url = "http://127.0.0.1:6334"
```

### 运行

```bash
# 启动网关
./target/release/crab-gateway config/gateway.toml

# 启动管理界面
./target/release/crab-admin
```

### 使用

```bash
# 发送请求
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-api-key" \
  -d '{
    "model": "deepseek-v4-pro",
    "messages": [{"role": "user", "content": "Hello, CrabCache!"}]
  }'
```

## ⚙️ 配置详解

### 缓存配置

```toml
[cache]
# L0 内存缓存配置
l0_max_capacity = 10000      # 最大缓存条目数
l0_ttl_secs = 3600           # TTL（秒）

# L1 Redis 配置
l1_redis_url = "redis://127.0.0.1:6379"
l1_pool_size = 16            # 连接池大小

# TTL 覆盖规则
[cache.model_ttl_overrides]
"deepseek-v4-pro" = 7200
"deepseek-v4-flash" = 3600

[cache.consumer_overrides]
"reporting-job" = 1800
```

### 语义缓存配置

```toml
[semantic]
enabled = true
model_path = "models/all-MiniLM-L6-v2.onnx"
tokenizer_path = "models/tokenizer.json"
qdrant_url = "http://127.0.0.1:6334"
collection_name = "crabcache"
similarity_threshold = 0.95  # 相似度阈值
ttl_secs = 86400            # TTL（秒）
```

### 推理配置

```toml
[reasoning]
thinking_mode = "enabled"           # enabled/disabled
reasoning_effort = "max"            # low/medium/high/max
missing_reasoning_strategy = "recover"  # recover/ignore/fail
display_reasoning = true            # 在 UI 中显示推理过程
collapsible_reasoning = true        # 可折叠的推理内容
cache_db_path = "data/reasoning_content.sqlite3"
```

## 📊 性能

### 测试结果

基于 **缓存命中率模拟测试方案 v1.0** 的测试结果：

| 指标 | 数值 | 目标 | 状态 |
|------|------|------|------|
| **请求级命中率** | 99.68% | > 95% | ✅ Pass |
| **Token级命中率** | 99.68% | > 90% | ✅ Pass |
| **P99 延迟** | 19.20ms | < 50ms | ✅ Pass |
| **P95 延迟** | 13.02ms | < 30ms | ✅ Pass |
| **延迟改善** | 98.04% | > 90% | ✅ Pass |

### 性能评级

🟢 **卓越** - 效率非常高，已达生产级优秀水准

### 缓存层级分布

- **L0 (内存缓存)**: 93.0%
- **L1 (Redis)**: 0.0%
- **L2 (语义缓存)**: 6.7%
- **未命中**: 0.32%

## 🧪 测试

### 运行测试

```bash
# 单元测试
cargo test --workspace

# 缓存命中率测试
cd tests
python3 crabcache_test_v1.py

# 查看测试报告
cat crabcache_test_report.md
```

### 测试覆盖

- ✅ 缓存命中率测试（L0/L1/L2）
- ✅ 请求合并测试
- ✅ 多密钥认证测试
- ✅ 语义相似度测试
- ✅ 性能基准测试

## 📈 监控

### Prometheus 指标

访问 `http://localhost:9090/metrics` 获取指标：

```promql
# 缓存命中率
sum(rate(gateway_cache_requests_total{result="hit"}[5m])) 
  / sum(rate(gateway_cache_requests_total[5m]))

# 各层级命中率
sum(rate(gateway_cache_requests_total{tier="L0",result="hit"}[5m]))

# Token 使用量
sum(rate(gateway_deepseek_input_tokens_total[1h]))

# 响应延迟 P99
histogram_quantile(0.99, 
  sum(rate(gateway_upstream_latency_seconds_bucket[5m])) by (le)
)
```

### Grafana 仪表板

导入 `grafana-dashboard.json` 获取预配置的监控面板。

## 🐳 Docker 部署

```bash
# 构建镜像
docker build -t crabcache:latest .

# 运行容器
docker-compose up -d

# 查看日志
docker-compose logs -f crab-gateway
```

### Docker Compose

```yaml
version: '3.8'
services:
  crab-gateway:
    image: crabcache:latest
    ports:
      - "8080:8080"
      - "9090:9090"
    volumes:
      - ./config:/app/config
      - ./data:/app/data
    depends_on:
      - redis
      - qdrant

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"

  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
      - "6334:6334"
```

## 🛠️ 开发

### 项目结构

```
CrabCache/
├── crates/
│   ├── crab-gateway/      # 主入口服务
│   ├── crab-proxy/        # ProxyHttp 实现
│   ├── crab-route/        # Ketama 路由
│   ├── crab-cache/        # L0/L1 精确缓存
│   ├── crab-semantic/     # L2 语义缓存
│   ├── crab-metrics/      # Prometheus 指标
│   ├── crab-reasoning/    # 推理内容处理
│   └── crab-admin/        # 管理界面后端
├── config/                # 配置文件
├── tests/                 # 测试脚本
└── models/                # ONNX 模型
```

### 编码规范

本项目严格遵循 Rust 编码规范：

- **rust-architecture-guide**: 架构设计和编码标准
- **rust-systems-cloud-infra-guide**: 云原生基础设施最佳实践

### 贡献指南

1. Fork 项目
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

## 📝 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 🙏 致谢

- [Pingora](https://github.com/cloudflare/pingora) - Cloudflare 开源的高性能网络框架
- [DeepSeek](https://www.deepseek.com/) - 提供优秀的 LLM API 服务
- [deepseek-cursor-proxy](https://github.com/yxlao/deepseek-cursor-proxy) - 推理内容处理参考实现

## 📞 联系方式

- **问题反馈**: [GitHub Issues](https://github.com/yourusername/crabcache/issues)
- **功能建议**: [GitHub Discussions](https://github.com/yourusername/crabcache/discussions)

---

<div align="center">

**[⬆ 返回顶部](#crabcache---deepseek-v4-高性能-rust-api-网关)**

Made with ❤️ by CrabCache Team

</div>
