# CrabCache - DeepSeek V4 高性能 Rust API 网关

<div align="center">

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Pingora](https://img.shields.io/badge/built%20with-Pingora-8B5CFE.svg)](https://github.com/cloudflare/pingora)

**一个基于 Cloudflare Pingora 框架的高性能 API 网关，专为 DeepSeek V4 大语言模型设计**

[特性](#特性) • [架构](#架构) • [快速开始](#快速开始) • [配置](#配置) • [管理系统](#管理系统) • [性能](#性能) • [测试](#测试)

</div>

---

## 📖 概述

CrabCache 是一个生产级的 Rust API 网关，通过三级缓存架构、会话亲和性路由和智能推理内容管理，实现极致的成本优化和低延迟响应。基于 Cloudflare Pingora 框架构建，提供亚毫秒级延迟和无锁连接池。

### 🎯 核心目标

- **成本优化**: 通过多级缓存减少 95%+ 的 API 调用成本
- **低延迟**: P99 延迟 < 1ms（缓存命中场景）
- **高可用**: 支持多密钥管理、请求合并、优雅降级
- **可观测**: 完整的 Prometheus 指标和结构化日志
- **推理管理**: 支持 DeepSeek 推理内容缓存与恢复

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
| **L2** | Qdrant 语义向量缓存 | P99 < 20ms | 5-10% |

### 🔄 智能路由

- **Ketama 一致性哈希**: 最大化 DeepSeek V4 前缀缓存命中率
- **会话亲和性**: `x-conversation-id` / `x-prompt-cache-key` / body `prompt_cache_key` 粘滞到同一 DeepSeek peer，配合上游 L3 前缀缓存
- **动态权重**: 支持后端节点动态扩缩容（通过管理 API 热更新）

### 🧠 推理内容管理

- **推理内容缓存**: SQLite 持久化缓存 DeepSeek 推理过程，避免重复计算
- **流式推理恢复**: SSE 流中实时检测和恢复缺失的推理内容
- **Cursor 兼容**: 支持 Cursor IDE 的推理内容显示协议（可折叠区块）
- **多种策略**: `recover` / `fill_only` / `reject`；`fill_only` 保持 messages 前缀稳定以提升 DeepSeek L3 命中率（见 [`docs/DEEPSEEK_PREFIX_CACHE.md`](docs/DEEPSEEK_PREFIX_CACHE.md)）

### 🛡️ 企业级特性

- **多密钥管理**: 支持多租户 API 密钥的动态创建、撤销和更新
- **请求合并 (Coalescing)**: 防止缓存击穿（Cache Stampede）
- **优雅降级**: 缓存/后端故障时自动降级
- **流式响应**: SSE 流式响应优化，旁路拦截缓存
- **管理 API**: 完整的 HTTP 控制面（健康检查、状态监控、配置热更新）
- **管理面板**: 基于 Leptos 的 Web 管理界面（crab-admin + crab-dashboard）
- **影子日志**: 匿名化日志采集，支撑缓存命中率分析与调优
- **SecretString**: API Key 脱敏处理，防止日志泄露

## 🏗️ 架构

```
                          ┌──────────────────────────────────────────┐
                          │              CrabCache Gateway            │
                          │  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
                          │  │ L0 Cache  │  │ L1 Cache  │  │ L2 Cache│ │
                          │  │  (Moka)   │  │  (Redis)  │  │(Qdrant) │ │
                          │  └──────────┘  └──────────┘  └─────────┘ │
                          │  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
                          │  │  Router   │  │Coalescer │  │ Metrics │ │
                          │  │ (Ketama)  │  │(Request) │  │Prometheus││
                          │  └──────────┘  └──────────┘  └─────────┘ │
                          │  ┌──────────┐  ┌──────────────────────┐  │
                          │  │Reasoning │  │  Management API      │  │
                          │  │  Store   │  │  :9080 (Control)     │  │
                          │  └──────────┘  └──────────────────────┘  │
                          └────────────────┬─────────────────────────┘
                                           │
                    ┌──────────────────────┼──────────────────────┐
                    │                      ▼                      │
                    │            ┌──────────────────┐             │
                    │            │  DeepSeek V4     │             │
                    │            │  API Upstream    │             │
                    │            └──────────────────┘             │
                    │                                              │
┌─────────────────────┐   ┌──────────────┐   ┌──────────────────┐  │
│ crab-admin :3000    │   │   Redis :6379 │   │  Qdrant :6334    │  │
│ (Admin Dashboard)   │   │  (L1 Cache)   │   │ (L2 Vector Store)│  │
└─────────────────────┘   └──────────────┘   └──────────────────┘  │
                                                                     │
┌─────────────────────┐   ┌──────────────┐                          │
│  Prometheus :9090   │   │  Grafana     │                          │
│  (Metrics)          │   │  (Dashboards)│                          │
└─────────────────────┘   └──────────────┘                          │
```

### 模块依赖关系

```
crab-gateway (入口 + 配置)
  ├── crab-proxy     (ProxyHttp 实现 + SSE 流处理)
  │   ├── crab-route     (Ketama 一致性哈希路由)
  │   ├── crab-cache     (L0 Moka + L1 Redis 缓存)
  │   ├── crab-semantic  (L2 Qdrant 语义缓存)
  │   ├── crab-reasoning (推理内容管理与恢复)
  │   └── crab-metrics   (Prometheus 指标采集)
  └── crab-control   (管理 API 客户端 + 共享类型)

crab-admin (管理面板后端)
  ├── crab-control  (Gateway 管理 API 客户端)
  └── crab-dashboard (Leptos WASM 前端)
```

## 🚦 快速开始

### 环境要求

- **Rust**: 2024 Edition (stable)
- **系统**: Linux 内核（生产环境必需，macOS 存在 SSE Bug）
- **依赖服务**: Redis, Qdrant（可选，用于 L2 语义缓存）

### 安装与构建

```bash
# 克隆仓库
git clone https://github.com/smile9493/CrabCache.git
cd CrabCache

# 构建项目（所有 crate）
cargo build --release

# 仅构建网关
cargo build --release -p crab-gateway

# 复制配置文件
cp config/gateway.example.toml config/gateway.toml
```

### 配置

编辑 `config/gateway.toml`，设置您的 DeepSeek API Key 和其他参数：

```toml
listen_addr = "0.0.0.0:8080"
metrics_addr = "0.0.0.0:9090"
api_key = "sk-your-deepseek-api-key"

[upstream]
# 上游中转根地址（不含 /v1，与 new-api 渠道 base_url 一致）
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
# 可省略，将从 base_url 自动推导 host:443
deepseek_endpoints = ["api.deepseek.com:443"]

[cache]
l0_max_capacity = 10000
l0_ttl_secs = 3600
l1_redis_url = "redis://127.0.0.1:6379"

[semantic]
enabled = false
```

### 启动依赖服务（裸机）

```bash
# L1 缓存必需：仅 Redis（本地）
docker compose up -d redis

# 可选 L2 语义缓存
docker compose --profile semantic up -d qdrant
```

### 运行网关

```bash
# 启动网关
cargo run --release -p crab-gateway -- config/gateway.toml

# 启动管理面板（可选）
cargo run --release -p crab-admin
```

网关默认监听：
- `:8080` — API 代理端口
- `:9090` — Prometheus 指标端口
- `:9080` — 管理 API（控制面，默认仅 loopback）
- `:3000` — 管理面板（crab-admin，默认 `0.0.0.0:3000`）

### 使用示例

```bash
# 发送聊天补全请求
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-your-deepseek-api-key" \
  -d '{
    "model": "deepseek-v4-pro",
    "messages": [{"role": "user", "content": "Hello, CrabCache!"}]
  }'

# 列表模型
curl http://localhost:8080/v1/models \
  -H "Authorization: Bearer sk-your-deepseek-api-key"

# 健康检查
curl http://localhost:8080/health
```

## ⚙️ 完整配置参考

完整的配置文件示例见 `config/gateway.example.toml`，以下为关键配置说明。

### 核心配置

```toml
listen_addr = "0.0.0.0:8080"        # 网关监听地址
metrics_addr = "0.0.0.0:9090"       # Prometheus 指标端口
api_key = "sk-xxxx"                  # DeepSeek API Key（也可通过 CRABCACHE_API_KEY 环境变量设置）
```

### 管理 API 配置

```toml
[management]
listen_addr = "127.0.0.1:9080"       # 管理 API 监听地址（默认仅 loopback）
admin_key = "your-admin-secret"      # 管理 API 认证密钥（也可通过 CRABCACHE_GATEWAY_ADMIN_KEY 设置）
```

### 缓存配置

```toml
[cache]
default_ttl_secs = 3600              # 默认缓存 TTL（秒）
l0_max_capacity = 10000              # L0 Moka 最大缓存条目数
l0_ttl_secs = 3600                   # L0 TTL（秒）
l1_redis_url = "redis://127.0.0.1:6379"  # Redis 连接 URL
l1_pool_size = 16                    # Redis 连接池大小
stream_cache_enabled = true          # 是否缓存流式响应
fingerprint_version = 1              # 缓存键指纹版本（可安全刷新缓存）
fingerprint_normalize_content = true # 是否对消息内容进行规范化

[cache.model_ttl_overrides]
"deepseek-v4-pro" = 7200
"deepseek-v4-flash" = 3600

[cache.consumer_overrides]
"reporting-job" = 1800
```

### 语义缓存配置

```toml
[semantic]
enabled = false                      # 是否启用 L2 语义缓存
model_path = "models/all-MiniLM-L6-v2.onnx"
tokenizer_path = "models/tokenizer.json"
qdrant_url = "http://127.0.0.1:6334" # Qdrant gRPC 端点
collection_name = "crab_semantic_cache"
similarity_threshold = 0.95          # 语义相似度阈值
ttl_secs = 86400                     # 语义缓存 TTL
```

### 推理内容管理

```toml
[reasoning]
thinking_mode = "enabled"            # enabled / disabled
reasoning_effort = "max"             # low / medium / high / max
missing_reasoning_strategy = "recover" # recover | reject
display_reasoning = true             # 在响应中展示推理过程
collapsible_reasoning = true         # 使用可折叠区块展示
cache_db_path = "data/reasoning_content.sqlite3"
cache_max_age_secs = 2592000         # 推理缓存最大存活时间（30天）
cache_max_rows = 100000              # 推理缓存最大条目数
```

### 连接配置

```toml
[connection]
tcp_keepalive_idle_secs = 60         # TCP Keepalive 空闲秒数
tcp_keepalive_interval_secs = 10     # TCP Keepalive 间隔
tcp_keepalive_count = 3              # TCP Keepalive 重试次数
idle_timeout_secs = 90               # 空闲连接超时
h2_ping_interval_secs = 30           # HTTP/2 Ping 间隔
```

### 影子日志配置

```toml
[trace_logging]
enabled = true                       # 是否启用影子日志
path = "/var/log/crabcache/trace.jsonl"
max_lines = 10000                    # 每文件最大行数
max_files = 5                        # 最大保留文件数
```

### 两把钥匙（部署必读）

CrabCache 使用**三套独立密钥**，不可混用：

| 配置 / 环境变量 | HTTP 头 | 用途 |
|-----------------|---------|------|
| Management 创建的 `sk-cc-*` | `Authorization: Bearer …` | **客户端**访问网关（Agent / IDE） |
| `upstream.keys` / `CRABCACHE_UPSTREAM_KEYS` / `CRABCACHE_API_KEY` | （仅服务端） | **上游** DeepSeek 配额池；网关出站轮换，勿发给客户端 |
| `[management].admin_key` / `CRABCACHE_GATEWAY_ADMIN_KEY` | `x-gateway-admin-key` | Management API（清缓存、密钥 CRUD、上游 Key 池等） |

可选：`[gateway].legacy_api_key_as_client_auth = true` 时仍允许用 `api_key` 当客户端 Bearer（不推荐生产）。

生产环境请同时更换两者。Docker 部署时务必设置 `CRABCACHE_GATEWAY_ADMIN_KEY`（示例配置中的 `dev-only-gateway-admin-secret` 仅用于本地开发）。启动时若仍为已知弱密钥，网关会输出 `Security warning` 日志。

### 环境变量覆盖

| 环境变量 | 作用 | 默认值 |
|----------|------|--------|
| `CRABCACHE_API_KEY` | 覆盖配置文件的 `api_key`（单 Key 兼容） | — |
| `CRABCACHE_UPSTREAM_KEYS` | 逗号分隔的上游 DeepSeek Key 池 | — |
| `CRABCACHE_GATEWAY_ADMIN_KEY` | 管理 API 认证密钥 | `change-me-in-production` |
| `CRABCACHE_MANAGEMENT_LISTEN` | 管理 API 监听地址 | `127.0.0.1:9080` |
| `CRABCACHE_ADMIN_KEY` | Admin Dashboard 认证密钥 | `admin` |
| `CRABCACHE_GATEWAY_CONTROL_URL` | Admin Dashboard 连接网关的管理 API 地址 | `http://127.0.0.1:9080` |
| `CRABCACHE_GATEWAY_CLIENT_PORT` | Keys 页展示的客户端网关端口 | `8080` |
| `CRABCACHE_GATEWAY_CLIENT_LAN_HOST` | 覆盖局域网网关主机（Docker 部署建议设置宿主机 LAN IP） | — |
| `CRABCACHE_GATEWAY_OPENRESTY_BASE_URL` | 覆盖 Keys 页 OpenResty 客户端 Base URL | — |
| `CRABCACHE_OPENRESTY_CONF_DIR` | 自动检测 OpenResty 时扫描的 Nginx 配置目录 | — |
| `CRABCACHE_GATEWAY_CLIENT_BASE_URL` | 同上（遗留别名） | — |
| `CRABCACHE_HTTPS` | Admin Dashboard 是否启用 HTTPS 模式 | `false` |
| `DEEPSEEK_API_KEY` | Admin Dashboard 同步模型时使用的 API Key | — |
| `RUST_LOG` | 日志级别 | `info` |

## 🛠️ 管理系统

### Management API（控制面）

网关内置 HTTP Management API（默认 `:9080`），用于运行时管理：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/health` | GET | 健康检查（无需认证） |
| `/v1/status` | GET | 网关状态：运行时间、活跃密钥数、后端数 |
| `/v1/keys` | GET | 列出所有 API Key |
| `/v1/keys` | POST | 创建新的 API Key |
| `/v1/keys/{token}` | DELETE | 撤销指定 API Key |
| `/v1/keys/{token}` | PATCH | 更新 Key 名称/启用状态 |
| `/v1/cache/ttl` | GET | 获取当前 TTL 配置 |
| `/v1/cache/ttl` | PUT | 更新 TTL 配置 |
| `/v1/routing/backends` | GET | 获取当前路由后端列表 |
| `/v1/routing/backends` | PUT | 动态更新路由后端 |

所有非 `/v1/health` 的端点需要通过 `X-Gateway-Admin-Key` 请求头认证。

```bash
# 创建 API Key
curl -X POST http://127.0.0.1:9080/v1/keys \
  -H "X-Gateway-Admin-Key: your-admin-secret" \
  -H "Content-Type: application/json" \
  -d '{"name": "my-app", "enabled": true}'

# 查看网关状态
curl -s http://127.0.0.1:9080/v1/status \
  -H "X-Gateway-Admin-Key: your-admin-secret" | jq
```

### Admin Dashboard（管理面板）

crab-admin 提供了一个基于 Leptos WASM 的 Web 管理界面。首次打开需在登录页输入与服务器 `CRABCACHE_ADMIN_KEY` 相同的 Admin API Key（请求头 `x-admin-key`）；侧栏可更改密钥。开发构建（debug）提供一键填入默认 `admin` 的快捷按钮。

```bash
# 启动管理面板（HTTP 模式）
cargo run --release -p crab-admin

# 启动管理面板（HTTPS 模式，需要自签名证书）
cargo run --release -p crab-admin -- --https

# 自定义端口
cargo run --release -p crab-admin -- --listen 0.0.0.0:3000
```

#### 命令行参数

| 参数 | 简写 | 说明 | 默认值 |
|------|------|------|--------|
| `--listen` | `-l` | 监听地址 | `0.0.0.0:3000` |
| `--cert` | `-c` | SSL 证书路径 | — |
| `--key` | `-k` | SSL 私钥路径 | — |
| `--https` | — | 启用 HTTPS（默认证书路径 `certs/`） | — |

#### 管理面板功能

- **API Key 管理**: 创建、查看、撤销 API Key（同步网关管理 API）
- **缓存配置**: 查看/更新 L0/L1 TTL、语义缓存相似度阈值
- **连接配置**: TCP Keepalive、空闲超时、H2 Ping 配置
- **上游配置**: 更新 DeepSeek Base URL、端点列表、API Key
- **模型管理**: 从上游同步模型列表、查看模型元数据
- **路由状态**: 查看当前 Ketama 后端分布
- **请求日志**: 实时请求日志查看、详细分析
- **Trace 分析**: 影子日志分析面板（请求重复率、语义聚类、Zipf 分布）
- **监控指标**: QPS、TPS、缓存命中率、延迟分布

## 📊 性能

### 缓存层次延迟

| 层级 | 技术 | P99 延迟 | 命中率 | 容量 |
|------|------|----------|--------|------|
| **L0** | Moka 内存缓存 | < 100ns | ~60-70% | 10,000 条目 |
| **L1** | Redis 分布式缓存 | < 5ms | ~20-30% | 取决于 Redis 内存 |
| **L2** | Qdrant 语义向量缓存 | < 20ms | ~5-10% | 百万级向量 |

### 综合测试结果

| 指标 | 数值 | 目标 | 状态 |
|------|------|------|------|
| **请求级命中率** | 99.68% | > 95% | ✅ |
| **Token 级命中率** | 99.68% | > 90% | ✅ |
| **P99 延迟** | 19.20ms | < 50ms | ✅ |
| **P95 延迟** | 13.02ms | < 30ms | ✅ |
| **延迟改善** | 98.04% | > 90% | ✅ |

### 缓存层级分布

- **L0 (Moka 内存)**: 93.0%
- **L1 (Redis)**: 0.0%
- **L2 (Qdrant 语义)**: 6.7%
- **未命中**: 0.32%

## 📈 监控

### Prometheus 指标

访问 `http://localhost:9090/metrics` 获取指标。

#### 核心指标

| 指标名 | 类型 | 标签 | 说明 |
|--------|------|------|------|
| `gateway_deepseek_input_tokens_total` | Counter | `cache_status`, `model`, `consumer` | DeepSeek 输入 Token 数 |
| `gateway_deepseek_output_tokens_total` | Counter | `model`, `consumer` | DeepSeek 输出 Token 数 |
| `gateway_cache_requests_total` | Counter | `tier`, `result` | 各层级缓存请求数（hit/miss） |
| `gateway_upstream_latency_seconds` | Histogram | `model` | 上游请求延迟 |
| `gateway_stream_first_token_latency_seconds` | Histogram | `model` | 首字延迟 (TTFT) |
| `gateway_cache_fetch_latency_seconds` | Histogram | `tier` | 缓存读取延迟 |
| `gateway_semantic_cache_requests_total` | Counter | `status` | 语义缓存请求状态 |
| `gateway_coalesced_requests_total` | Counter | — | 请求合并计数 |
| `gateway_cache_cost_saved_usd_total` | Counter | `model`, `consumer`, `tier` | 缓存节省费用 |
| `gateway_upstream_prompt_cache_tokens_total` | Counter | `status`, `model`, `consumer` | 上游 prompt 缓存 Token |

#### 常用 PromQL 查询

```promql
# 综合缓存命中率
sum(rate(gateway_cache_requests_total{result="hit"}[5m]))
  / sum(rate(gateway_cache_requests_total[5m]))

# L0 层级命中率
sum(rate(gateway_cache_requests_total{tier="L0_moka",result="hit"}[5m]))
  / sum(rate(gateway_cache_requests_total{tier="L0_moka"}[5m]))

# 上游延迟 P99
histogram_quantile(0.99,
  sum(rate(gateway_upstream_latency_seconds_bucket[5m])) by (le)
)

# Token 节省成本
sum(increase(gateway_cache_cost_saved_usd_total[24h]))
```

## 🐳 Docker 部署（Agent 中间层）

默认栈：**gateway + Redis**（`config/gateway.docker.toml`，语义缓存关闭）。Redis 不映射到宿主机公网端口。

```bash
cp .env.example .env
# 编辑 .env：
#   CRABCACHE_API_KEY 或 CRABCACHE_UPSTREAM_KEYS（上游 DeepSeek 密钥池）
#   CRABCACHE_GATEWAY_ADMIN_KEY（Management API）
# 可选：CRABCACHE_UPSTREAM_BASE_URL / CRABCACHE_UPSTREAM_MODEL（覆盖 TOML 中的中转地址）

docker compose up -d --build
docker compose ps   # gateway 应为 healthy（/ready 依赖 Redis）

# 创建客户端 sk-cc-*，再验收（勿把 DeepSeek 密钥当 CLIENT_API_KEY）
export CLIENT_API_KEY=sk-cc-...   # 来自 POST /v1/keys
./scripts/verify_deployment.sh    # 含流式 SSE：不得含 reasoning_content
# 公网域名+端口：CLIENT_API_KEY=sk-... bash scripts/verify_domain_port.sh
```

Cursor + DeepSeek 对照说明：[`docs/DEEPSEEK_CURSOR_PROXY_PARITY.md`](docs/DEEPSEEK_CURSOR_PROXY_PARITY.md)、[`docs/CURSOR_SETUP.md`](docs/CURSOR_SETUP.md)。

公网入口：用 Nginx 反代本机 `127.0.0.1:8080`，参考 [`deploy/nginx/crabcache-api.conf.example`](deploy/nginx/crabcache-api.conf.example)（需 `proxy_buffering off` 以支持流式）。**不要**将 Management `:9080` 或 Redis 暴露到公网。

1Panel + OpenResty 发布步骤与示例域名配置见 [`docs/deploy-1panel-openresty.md`](docs/deploy-1panel-openresty.md)、[`deploy/nginx/crabcache-openresty-1panel.example.conf`](deploy/nginx/crabcache-openresty-1panel.example.conf)。

### Agent 客户端配置

| 字段 | 值 |
|------|-----|
| Base URL | `https://你的域名/v1`（OpenAI SDK 会自动请求 `/chat/completions`） |
| API Key | Management `POST /v1/keys` 颁发的 `sk-cc-*`（推荐） |
| Model | 请求体中的 `model` 字段（如 `deepseek-v4-pro`） |

创建客户端密钥（在服务器上，Management 默认容器内 `0.0.0.0:9080`）：

```bash
docker compose exec gateway curl -s -X POST http://127.0.0.1:9080/v1/keys \
  -H "x-gateway-admin-key: ${CRABCACHE_GATEWAY_ADMIN_KEY}" \
  -H "Content-Type: application/json" \
  -d '{"name":"agent-1","enabled":true}'
```

### Docker Compose 服务

| 服务 | 默认启动 | 宿主机端口 | 说明 |
|------|----------|------------|------|
| `gateway` | 是 | `127.0.0.1:8080`, `127.0.0.1:9090` | Agent API + Management（9080 仅容器内） |
| `redis` | 是 | 无（仅 Docker 网络） | L1 缓存 |
| `qdrant` | `--profile semantic` | 无 | L2 可选 |

环境变量见 [`.env.example`](.env.example)。可选生产覆盖：[`docker-compose.prod.yml`](docker-compose.prod.yml)。若需接入外部 OpenResty 网络：`docker compose -f docker-compose.yml -f docker-compose.openresty.yml up -d`。

**上游中转（热更新）**：Management `GET/PUT /v1/upstream/relay`（`base_url`、`model`、`endpoints`）；Admin 面板上游配置会同步到网关运行时。

**Cursor + DeepSeek thinking**：参见 [`docs/CURSOR_SETUP.md`](docs/CURSOR_SETUP.md)。**上游 L3 前缀缓存**：参见 [`docs/DEEPSEEK_PREFIX_CACHE.md`](docs/DEEPSEEK_PREFIX_CACHE.md)。清空 reasoning SQLite：`crab-gateway --clear-reasoning-cache config/gateway.toml` 或 `DELETE /v1/reasoning/cache`。

**Admin 辅助持久化**：`CRABCACHE_ADMIN_STATE_PATH`（默认 `data/admin-state.json`）保存模型列表元数据、上次连通性测试结果等；运行时 relay 与 Key 池以 Gateway 为准，Admin 启动时会与 Gateway 对齐。

可选 Admin Dashboard（需先 [`scripts/build_dashboard.sh`](scripts/build_dashboard.sh) 构建前端）：

```bash
docker compose --profile admin up -d --build
# http://127.0.0.1:3000  (CRABCACHE_ADMIN_KEY)
```

客户端 Key 迁移说明：[`docs/AGENT_CLIENT_KEY_MIGRATION.md`](docs/AGENT_CLIENT_KEY_MIGRATION.md)。Prometheus 告警示例：[`deploy/prometheus/alerts.example.yml`](deploy/prometheus/alerts.example.yml)。

> **注意**: 生产环境建议 Prometheus/Grafana 采集 `9090` 指标（绑定本机，勿对公网开放）。

## 🧪 测试

```bash
# 运行所有单元测试
cargo test --workspace

# 运行集成测试（CI 自带 Redis；本地可先 docker compose up -d redis）
cargo test -p crab-cache -p crab-gateway

# 运行 clippy 检查
cargo clippy --workspace -- -D warnings

# 代码格式化检查
cargo fmt --check
```

### 测试覆盖

| Crate | 覆盖范围 |
|-------|---------|
| `crab-metrics` | 指标注册、枚举转换、记录方法 |
| `crab-route` | 一致性哈希、节点漂移率、亲和性键提取 |
| `crab-cache` | TTL 配置、缓存键生成、请求合并 (coalescing) |
| `crab-proxy` | 上下文构造、语义查询文本构建、SSE 解析 |
| `crab-gateway` | 配置加载、管理 API 端点集成测试 |
| `crab-control` | 后端端点解析 |
| `crab-semantic` | 模拟嵌入和搜索测试 |

## 📁 项目结构

```
CrabCache/
├── Cargo.toml                     # Workspace 根配置
├── config/
│   ├── gateway.example.toml       # 配置模板
│   └── gateway.toml               # 实际配置（忽略）
├── crates/
│   ├── crab-gateway/             # 主入口服务（Pingora Server）
│   │   ├── src/main.rs           # 入口：Server 构建、模块组装
│   │   ├── src/config.rs         # 配置结构体 + 验证 + 环境变量覆盖
│   │   ├── src/management.rs     # 管理 HTTP API（axum）
│   │   └── tests/management_api.rs  # 管理 API 集成测试
│   ├── crab-proxy/               # ProxyHttp 实现
│   │   ├── src/proxy.rs          # ProxyHttp trait 实现（~1000行）
│   │   ├── src/context.rs        # GatewayContext（请求级上下文）
│   │   ├── src/runtime.rs        # RuntimeConfig（共享运行时配置）
│   │   ├── src/error.rs          # ProxyError 类型
│   │   ├── src/sse.rs            # SSE 流式响应解析
│   │   └── src/trace_logger.rs   # 影子日志（脱敏日志记录）
│   ├── crab-route/               # Ketama 一致性哈希路由
│   │   ├── src/ring.rs           # AffinityRouter 封装
│   │   └── src/affinity.rs       # 亲和性键提取策略
│   ├── crab-cache/               # L0/L1 精确缓存
│   │   ├── src/tiered.rs         # TieredCache（Moka + Redis）
│   │   ├── src/types.rs          # CacheEntry, TtlConfig, UsageInfo
│   │   ├── src/key.rs            # 缓存键生成（SHA256 + 规范化）
│   │   ├── src/coalescing.rs     # 请求合并（防缓存击穿）
│   │   ├── src/hit_rate_sim.rs   # 缓存命中率模拟器
│   │   ├── src/trace_analyzer.rs # 影子日志分析
│   │   └── src/trace_loader.rs   # Trace 加载与场景生成
│   ├── crab-semantic/            # L2 语义缓存
│   │   ├── src/cache.rs          # SemanticCache 协调器
│   │   ├── src/embedder.rs       # ONNX Runtime (ort) 推理引擎
│   │   └── src/store.rs          # Qdrant 向量存储 CRUD
│   ├── crab-reasoning/           # 推理内容管理
│   │   ├── src/lib.rs            # 推理内容缓存与恢复
│   │   └── src/...               # 规范化、SSE 流改写等
│   ├── crab-metrics/             # Prometheus 指标
│   │   └── src/registry.rs       # 全局 GatewayMetrics 注册
│   ├── crab-control/             # 共享类型 & 管理 API 客户端
│   │   ├── src/types.rs          # 共享 DTO 类型
│   │   ├── src/client.rs         # GatewayAdminClient（HTTP 客户端）
│   │   ├── src/backends.rs       # 后端端点解析
│   │   └── src/error.rs          # ControlError 类型
│   ├── crab-admin/               # 管理面板后端
│   │   ├── src/main.rs           # Axum 服务器（HTTP/HTTPS）
│   │   ├── src/routes.rs         # Admin API 端点
│   │   ├── src/state.rs          # 应用状态与存储
│   │   └── src/types.rs          # 响应类型
│   └── crab-dashboard/           # Leptos WASM 前端
│       └── src/                  # 前端组件与页面
├── docs/
│   ├── network-config.md         # 网络配置指南
│   └── self-signed-cert.md       # 自签名证书指南
├── scripts/
│   ├── generate-cert.sh          # SSL 证书生成脚本
│   ├── start-https.sh            # HTTPS 启动脚本
│   ├── start-http.sh             # HTTP 启动脚本
│   ├── analyze_trace.py          # 影子日志分析脚本
│   └── collect_trace.sh          # 日志采集脚本
├── Dockerfile                    # Docker 构建
└── docker-compose.yml            # Docker Compose
```

## 🛠️ 开发

### 编码规范

本项目严格遵循 Rust 编码规范：

- **rust-architecture-guide**: 架构设计和编码标准（优先级金字塔 P0-安全 > P1-可维护性 > P2-编译时间 > P3-性能）
- **rust-systems-cloud-infra-guide**: 云原生基础设施最佳实践（I/O 模型、零拷贝、背压机制）

### 重要开发注意事项

#### 1. 部署平台限制
⚠️ **生产环境必须运行于 Linux 内核**

Pingora 在 macOS 上存在 SSE 流式响应 Bug (Issue #841)，导致数据无法即时 flush 到客户端。
- 生产部署：直接运行于 Linux 服务器
- 开发调试：使用 Docker 容器（网络协议栈隔离）

#### 2. ONNX 模型文件
`all-MiniLM-L6-v2.onnx` 需要从 Hugging Face 下载（约 80MB），不提交到 Git：
```bash
mkdir -p models
# 从 Hugging Face 下载 all-MiniLM-L6-v2.onnx 和 tokenizer.json 到 models/ 目录
```

#### 3. Git 与本地工具链目录

请勿在仓库根目录安装 Rust 工具链（避免产生 `.rustup/`、`.cargo/` 等数万未跟踪文件）。应使用系统默认路径：

```bash
# 推荐：工具链在用户主目录
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
```

构建产物位于 `target/`（已在 `.gitignore` 中）。若误在仓库内生成 `.rustup/`，删除该目录即可：`rm -rf .rustup .cargo`。

#### 4. 内存分配器
生产环境使用 jemalloc：
```rust
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

## 🤝 贡献指南

1. Fork 项目
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

确保提交前通过：
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `cargo fmt --check`

## 📝 许可证

MIT License

## 🙏 致谢

- [Pingora](https://github.com/cloudflare/pingora) - Cloudflare 开源的高性能网络框架
- [DeepSeek](https://www.deepseek.com/) - 提供优秀的 LLM API 服务
- [Moka](https://github.com/moka-rs/moka) - 高性能 Rust 缓存库
- [Qdrant](https://qdrant.tech/) - Rust 原生向量数据库
- [ONNX Runtime](https://onnxruntime.ai/) - 跨平台推理引擎
- [Leptos](https://leptos.dev/) - Rust Web 框架
- [Axum](https://github.com/tokio-rs/axum) - Web 框架
