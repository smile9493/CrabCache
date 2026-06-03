# CrabCache - 多供应商 LLM API 高性能网关

## 项目概述

CrabCache 是一个基于 Cloudflare Pingora 框架构建的高性能 Rust API 网关，面向多供应商 LLM（大语言模型）API 的缓存优化与可观测透明分析。内置 **120+ 个 LLM 供应商**（OpenAI、Anthropic、DeepSeek、Groq、xAI、Mistral、Gemini、阿里通义、百度千帆、智谱 GLM 等），以 DeepSeek V4 为重点参考实现（当前唯一支持 Reasoning/thinking 的供应商）。通过多级缓存架构（L0/L1/L2）、会话亲和性路由和请求合并（Coalescing），实现极致的成本优化和低延迟响应。

### 核心特性

- **基于 Pingora 的高性能代理**：亚毫秒级延迟，无锁连接池
- **三级缓存防御体系**：
  - L0: Moka 进程内存缓存（P99 < 100ns）
  - L1: Redis 分布式精确缓存（P99 < 5ms）
  - L2: Qdrant 语义向量缓存（相似度阈值 0.95，含模型守卫）
- **Ketama 一致性哈希路由**：会话亲和，最大化上游前缀缓存命中率
- **请求合并 (Request Coalescing)**：同一缓存键的并发请求合并为一次上游调用，Leader/Follower 模式
- **API 密钥管理**：动态密钥创建/吊销/启停，DashMap 存储，支持消费者标签
- **Management HTTP API**：运行时管理——密钥 CRUD、TTL 动态调整、后端路由热更新
- **DeepSeek Reasoning 处理管线**：思考链提取、SSE 块改写、Cursor 折叠显示适配、SQLite 缓存
- **SSE 流式响应优化**：缓存命中时合成 SSE 流返回，流式响应可选缓存
- **缓存键指纹 (Fingerprint)**：版本化、Unicode NFC 标准化，安全失效旧缓存
- **多租户隔离**：`project_id` / `X-Project-Id` → 上游 `user_id` 映射 + 动态缓存命名空间（见 [docs/MULTI_TENANT.md](docs/MULTI_TENANT.md)）
- **配置验证**：启动时全面校验配置合法性（API Key、端点、地址等）
- **SecretString 安全处理**：密钥自动遮盖，杜绝日志泄漏
- **Prometheus 可观测性**：Token 成本追踪、延迟监控、成本节省估算
- **Trace 日志**：结构化 JSONL 文件记录，支持加载分析和命中率模拟
- **Admin Dashboard**：Leptos WASM 前端 + Axum 后端，提供图形化管理界面
- **Admin 冷存储**：上游 Key 池、OAuth 凭证、客户端 Key 元数据等 **冷数据以 PostgreSQL 为权威**（见 [docs/ADMIN_COLD_STORAGE.md](docs/ADMIN_COLD_STORAGE.md)）；Gateway Redis 仅为热管道
- **数据面优化（可选 `[features]`）**：
  - **Prefix-aware L0**：共享消息前缀仅更新索引并继续上游（不短路返回完整缓存）
  - **连接池直预热**：`connection_prewarm` 通过 Pingora 共享 `Connector` 做 TCP+TLS 握手入池（无 loopback HTTP）；新 `session_fingerprint` 在 `upstream_peer` 选路后触发
  - **Affinity 反馈**：`affinity_prompt_cache_feedback` 在请求结束时根据上游 `prompt_cache_*` token 更新 Ketama hint（连续 3 次 pure miss 才失效）
  - **MiMo 快速路径**：`body_quick_parse` + 全量 JSON 解析前 exact cache 探测，降低重复请求解析开销
  - **请求阶段指标**：`gateway_request_phase_latency_seconds`（body_read、json_parse、pipeline_select、cache_lookup、upstream_*、cache_write 等）
  - **SSE Pipeline**：Reasoning 改写 / SilentStrip / Passthrough 抽象（见 `crates/crab-proxy/src/sse_pipeline/`）
  - 实现状态总表见 [docs/DATA_PLANE.md](docs/DATA_PLANE.md)；P3 实验项见 [docs/DATA_PLANE_P3.md](docs/DATA_PLANE_P3.md)（`delta_cache`、`wasm_filters`、`io_uring_backend`，默认关闭）
  - 运行时 Trace 对照、MiMo 429 P0、非数据面遗漏见 [docs/OPS_RUNBOOK.md](docs/OPS_RUNBOOK.md)

## 技术栈

### 核心框架
- **Pingora 0.8**: Cloudflare 开源的高性能网络框架
- **Axum 0.8**: Management API 和 Admin Dashboard HTTP 框架
- **Tokio**: 异步运行时
- **Rust 2024 Edition**

### 缓存系统
- **Moka 0.12**: 高并发内存缓存（TinyLFU 策略）
- **Redis 7**: 分布式精确缓存（bb8 连接池）
- **Qdrant**: Rust 原生向量数据库（gRPC 接口）

### AI/ML
- **ONNX Runtime (ort 2.0.0-rc.12)**: 嵌入模型推理
- **all-MiniLM-L6-v2**: 语义向量化模型（384 维）
- **tokenizers**: HuggingFace 分词器

### 可观测性
- **Prometheus**: 指标采集（自定义 MetricsServer BackgroundService）
- **tracing**: 结构化日志（JSON 文件 + 控制台双输出）
- **tracing-appender**: 日志轮转

### Web 前端
- **Leptos 0.7**: WASM 前端框架（CSR 模式）
- **leptos_router / leptos_meta**: 路由和元数据

## 项目结构

```
CrabCache/
├── Cargo.toml                       # Workspace 配置（13 个 crates）
├── config/
│   └── gateway.example.toml         # 运行时配置模板
├── docker-compose.yml               # Docker 编排（gateway + redis + qdrant）
├── crates/
│   ├── crab-gateway/                # 主入口服务（Pingora Server + Management API）
│   │   ├── src/main.rs              # 启动入口
│   │   ├── src/lib.rs               # 库入口
│   │   ├── src/config.rs            # 配置加载、验证、SecretString
│   │   ├── src/management.rs        # Management HTTP API（axum router）
│   │   └── tests/management_api.rs  # Management API 集成测试
│   ├── crab-proxy/                  # ProxyHttp 实现
│   │   ├── src/lib.rs
│   │   ├── src/proxy.rs             # GatewayProxy：请求处理全流程
│   │   ├── src/context.rs           # GatewayContext / GatewayState / StoredKey
│   │   ├── src/runtime.rs           # RuntimeConfig（运行时可变配置）
│   │   ├── src/sse.rs               # SSE 解析、UsageData 提取
│   │   ├── src/sse_pipeline/        # 流式 SSE 管道（Reasoning / Passthrough 等）
│   │   ├── src/connection_prewarm.rs # 共享连接池 TCP+TLS 直预热
│   │   ├── src/body_quick_parse.rs  # 请求体快速字段提取（MiMo 惰性解析）
│   │   ├── src/error.rs             # ProxyError 枚举
│   │   └── src/trace_logger.rs      # 脱敏 Trace 日志（JSONL 文件）
│   ├── crab-route/                  # Ketama 路由
│   │   ├── src/ring.rs              # AffinityRouter（一致性哈希环）
│   │   └── src/affinity.rs          # 亲和性键提取
│   ├── crab-cache/                  # L0/L1 精确缓存
│   │   ├── src/tiered.rs            # TieredCache（L0 Moka + L1 Redis）
│   │   ├── src/coalescing.rs        # RequestCoalescer / CoalesceGuard
│   │   ├── src/key.rs               # 缓存键生成（fingerprint, namespace）
│   │   ├── src/types.rs             # CacheEntry / TtlConfig / UsageInfo
│   │   ├── src/hit_rate_sim.rs      # 命中率模拟器
│   │   ├── src/trace_loader.rs      # 日志加载 / 回放
│   │   ├── src/trace_analyzer.rs    # 日志分析 / 统计
│   │   └── src/sanitized_trace.rs   # 脱敏日志与参数拟合
│   ├── crab-semantic/               # L2 语义缓存
│   │   ├── src/embedder.rs          # ONNX 嵌入模型推理
│   │   ├── src/store.rs             # Qdrant 向量存储
│   │   └── src/cache.rs             # SemanticCache（搜索 + 插入）
│   ├── crab-metrics/                # Prometheus 指标
│   │   └── src/registry.rs          # GatewayMetrics + global_metrics()
│   ├── crab-reasoning/              # DeepSeek Reasoning 处理管线
│   │   ├── src/normalize.rs         # 请求准备、消息规范化
│   │   ├── src/streaming.rs         # 流式 SSE 改写、Cursor 适配器
│   │   ├── src/transform.rs         # 响应体重写、SSE chunk 改写
│   │   ├── src/keys.rs              # Reasoning 键生成
│   │   └── src/store.rs             # ReasoningStore（SQLite 存储）
│   ├── crab-pipeline/               # 请求管线选择与多供应商路由
│   │   ├── src/lib.rs               # Pipeline 类型、select_request_pipeline
│   │   ├── src/select.rs            # 管线选择逻辑
│   │   ├── src/types.rs             # PipelineOverride / PipelineRequestContext
│   │   ├── src/profile.rs           # 上游配置档描述
│   │   ├── src/cursor_models.rs     # Cursor 模型别名表（gpt-4o→deepseek-v4-pro 等）
│   │   └── src/signals.rs           # 信号定义
│   ├── crab-state/                  # Redis 控制面状态（密钥/策略持久化）
│   │   ├── src/lib.rs               # RedisStateStore
│   │   ├── src/redis_store.rs       # snapshot 序列化/反序列化
│   │   ├── src/sync.rs              # apply_snapshot_to_runtime, spawn_state_refresh_task
│   │   └── src/snapshot.rs          # 快照类型定义
│   ├── crab-control/                # 控制平面类型和客户端
│   │   ├── src/types.rs             # 共享 API 类型
│   │   ├── src/backends.rs          # 后端端点解析
│   │   ├── src/client.rs            # GatewayAdminClient（HTTP 客户端）
│   │   └── src/error.rs             # ControlError
│   ├── crab-admin/                  # Admin Dashboard 后端
│   │   ├── src/main.rs              # Axum 服务器（支持 HTTPS）
│   │   ├── src/routes.rs            # 管理 API 路由 + 审计日志
│   │   ├── src/state.rs             # AppState 和存储结构
│   │   ├── src/pg.rs                # PostgreSQL 存储（15 张表）
│   │   ├── src/domain_usage_sync.rs # domain_usage PG 同步任务
│   │   ├── src/types.rs             # API 请求/响应类型
│   │   └── src/network.rs           # 网络信息
│   ├── crab-dashboard/              # Leptos WASM 前端
│   │   ├── Cargo.toml               # Leptos 0.7 CSR 配置
│   │   └── src/                     # WASM 前端源码
│   └── crab-cli/                    # 运维 CLI（trace/日志/metrics 分析）
│       ├── Cargo.toml               # clap 4 + serde + regex
│       └── src/                     # CLI 入口 + 子命令模块
└── .cursor/skills/                  # Rust 编码规范 Skills
    ├── rust-architecture-guide/
    ├── rust-systems-cloud-infra-guide/
    └── rust-wasm-frontend-infra-guide/
```

## 编码规范

本项目严格遵循已配置的 Rust 编码规范 Skills：

### 1. rust-architecture-guide (v9.1.0)
- **优先级金字塔**: P0 安全 > P1 可维护性 > P2 编译时间 > P3 性能
- **执行模式**: `rapid` (原型) / `standard` (默认) / `strict` (生产)
- **所有权策略**: 业务层 Owned + `.clone()`，热路径 `Cow`/`Bytes` 零拷贝
- **错误处理**: 库级 `thiserror`，应用级 `anyhow`
- **编码哲学**: 截拳道 - 拦截样板代码，动作经济性，硬件亲和性

### 2. rust-systems-cloud-infra-guide (v6.1.0)
- **I/O 模型**: Tokio epoll vs io_uring 决策树
- **零拷贝管道**: `bytes::Bytes` O(1) 克隆
- **背压机制**: 有界通道 + Semaphore + 503 传播
- **确定性状态机**: 禁止 `Instant::now()`/`rand`/`HashMap` 迭代顺序
- **高级内存架构**: Arena (`bumpalo`) / Slab 预分配 / NUMA 感知

### 3. rust-wasm-frontend-infra-guide (v4.1.0)
- 如需 WebAssembly 支持，遵循此规范

## 开发指南

### 环境要求

- **Rust**: 2024 Edition (stable)
- **系统**: Linux 内核（生产环境必需，macOS 存在 SSE Bug）
- **依赖服务**: Redis（必需 L1 缓存）、Qdrant（可选，L2 语义缓存）

### 构建与运行

```bash
# 开发模式
cargo build

# 生产构建
cargo build --release

# 仅构建运维 CLI（不依赖 pingora，编译快）
cargo build --release -p crab-cli

# 运行测试
cargo test --workspace

# 代码检查
cargo clippy --workspace
cargo fmt --check
```

### 配置文件

复制并编辑配置文件：
```bash
cp config/gateway.example.toml config/gateway.toml
```

关键配置项：
- `listen_addr`: 网关监听地址（默认 0.0.0.0:8080）
- `metrics_addr`: Prometheus 指标端口（默认 0.0.0.0:9090）
- `[management]`: Management API 地址和密钥（默认 127.0.0.1:9080）
- `[upstream]`: 上游供应商配置（base_url、model、端点列表、TLS SNI）
- `[cache]`: L0/L1 缓存配置（容量、TTL、Redis 连接、Fingerprint、命名空间）
- `[cache.model_ttl_overrides]`: 按模型 TTL 覆盖
- `[cache.consumer_overrides]`: 按消费者 TTL 覆盖
- `[semantic]`: 语义缓存配置（ONNX 模型路径、Qdrant 地址、相似度阈值）
- `[reasoning]`: Reasoning 处理配置（思考模式、恢复策略、SQLite 缓存路径）
- `[connection]`: TCP/H2 连接参数（keepalive、idle timeout、ping）
- `[trace_logging]`: Trace 日志配置（JSONL 路径、行数限制）
- `[features]`: 实验性数据面开关（见 `config/gateway.example.toml` 注释）；`connection_prewarm` 依赖 `third_party/pingora-proxy` 的 `Arc<Connector>` 共享（见 `PATCH.md`）

### 环境变量覆盖

配置项可通过环境变量覆盖：
- `CRABCACHE_API_KEY`: 覆盖 `api_key`
- `CRABCACHE_GATEWAY_ADMIN_KEY`: 覆盖 `[management].admin_key`
- `CRABCACHE_MANAGEMENT_LISTEN`: 覆盖 `[management].listen_addr`
- `CRABCACHE_GATEWAY_CONTROL_URL`: Admin Dashboard 连接网关 Management API 的基础 URL
- `CRABCACHE_GATEWAY_CLIENT_PORT` / `CRABCACHE_GATEWAY_CLIENT_LAN_HOST`: Keys 页本地与局域网网关地址
- `CRABCACHE_GATEWAY_OPENRESTY_BASE_URL` / `CRABCACHE_OPENRESTY_CONF_DIR`: Keys 页 OpenResty 反代地址（可自动解析 1Panel `conf.d` 中指向 `127.0.0.1:8080` 的 server 块）

### 运行

```bash
# 直接运行网关
cargo run --bin crab-gateway -- config/gateway.toml

# 运行 Admin Dashboard（需要先启动网关）
cargo run --bin crab-admin

# Docker Compose 一键启动
docker-compose up -d
```

### 重要注意事项

#### 1. 部署平台限制
⚠️ **生产环境必须运行于 Linux 内核**

Pingora 在 macOS 上存在 SSE 流式响应 Bug (Issue #841)，导致数据无法即时 flush 到客户端。
- **生产部署**: 直接运行于 Linux 服务器
- **开发调试**: 使用 Docker 容器（网络协议栈隔离）

#### 2. ONNX 模型文件
`all-MiniLM-L6-v2.onnx` 需要从 Hugging Face 下载：
```bash
# 下载模型到 models/ 目录
mkdir -p models
# 从 Hugging Face 下载 all-MiniLM-L6-v2.onnx 和 tokenizer.json
```

模型文件约 80MB，不应提交到 Git。

#### 3. 内存分配器
生产环境使用 jemalloc 以提升高并发性能（非 MSVC 目标自动启用）：
```rust
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

#### 4. Docker 部署
`docker-compose.yml` 包含三个服务：
- `gateway`: 构建当前目录并运行
- `redis`: Redis 7 Alpine（带健康检查）
- `qdrant`: Qdrant 向量数据库（gRPC 端口 6334）

在 Docker 中需设置 `CRABCACHE_MANAGEMENT_LISTEN=0.0.0.0:9080` 以允许 Admin Dashboard 访问 Management API。

## 架构设计

### 请求处理流程

```
客户端请求
  │
  ├─ request_filter 阶段 ─────────────────────────────┐
  │   ├─ 健康检查端点 (/health, /healthz) → 200        │
  │   ├─ 模型列表端点 (GET /models, /v1/models) → 透传   │
  │   ├─ 路径验证（仅 /v1/chat/completions）             │
  │   ├─ 认证（Bearer Token → DashMap 或 bootstrap_key）│
  │   ├─ 请求体解析 → JSON → model/stream/conversation  │
  │   ├─ Reasoning 请求预处理（prepare_upstream_request）│
  │   ├─ 缓存键生成（Fingerprint + Namespace）          │
  │   ├─ L0/L1 缓存查找 → 命中 → 返回 SSE/JSON        │
  │   ├─ L2 语义缓存查找 → 命中（模型守卫检查）→ 返回   │
  │   └─ Request Coalescing（Leader 放行，Follower 等待）│
  │                                                     │
  ├─ upstream_peer 阶段 ────────────────────────────────┤
  │   ├─ 提取客户端亲和性键（IP / x-request-affinity）    │
  │   └─ Ketama 一致性哈希选择后端                        │
  │                                                     │
  ├─ upstream_request_filter / request_body_filter ─────┤
  │   └─ 替换请求体（Reasoning 预处理后）                  │
  │                                                     │
  ├─ response_filter 阶段 ──────────────────────────────┤
  │   ├─ 注入 x-request-id / x-cache-status             │
  │   └─ 记录上游启动时间                                 │
  │                                                     │
  ├─ upstream_response_body_filter 阶段 ────────────────┤
  │   ├─ 非流式：累积响应体 → EOS 时写入缓存（L0+L1+L2） │
  │   ├─ 流式：逐 chunk 处理 SSE 改写 + Reasoning 恢复  │
  │   │   ├─ TTFT 记录                                   │
  │   │   ├─ rewrite_sse_chunk（Reasoning→content）     │
  │   │   ├─ SSE UsageData 提取和指标记录                │
  │   │   └─ EOS 时合成消息并写入缓存（条件：stream_cache│
  │   └─ Response Body 改写（rewrite_response_body）    │
  │                                                     │
  └─ logging 阶段 ──────────────────────────────────────┘
      ├─ 结构化日志（请求 ID、延迟、缓存状态、Token 数）
      └─ Trace 日志（脱敏 → JSONL 文件）
```

### 缓存键生成

缓存键由以下组件构成：
1. **Fingerprint 版本号** (`fingerprint_version`): 递增版本可安全失效旧缓存
2. **内容标准化** (`fingerprint_normalize_content`): 去除冗余空白、`\r\n`→`\n`、Unicode NFC
3. **命名空间前缀** (`cache_key_namespace`): 可选的多租户隔离前缀
4. **请求体 SHA256 哈希**: 标准化后的消息内容哈希

```rust
// key 格式（无 namespace）: {fingerprint_version}:{sha256_hex}
// key 格式（有 namespace）: {namespace}:{fingerprint_version}:{sha256_hex}
```

### Management HTTP API

Management API 监听在 `[management].listen_addr`（默认 `127.0.0.1:9080`），通过 `x-gateway-admin-key` 头认证。提供以下端点：

| 方法 | 路径 | 描述 |
|------|------|------|
| GET | `/v1/health` | 健康检查（无需认证） |
| GET | `/v1/status` | 网关状态（运行时间、活跃密钥数、后端数） |
| GET | `/v1/keys` | 列出所有 API 密钥 |
| POST | `/v1/keys` | 创建新密钥（可指定 token，否则自动生成 `sk-cc-*`） |
| DELETE | `/v1/keys/{token}` | 吊销密钥 |
| PATCH | `/v1/keys/{token}` | 更新密钥（名称、启用状态） |
| GET | `/v1/cache/ttl` | 获取缓存 TTL 配置 |
| PUT | `/v1/cache/ttl` | 更新缓存 TTL 配置（动态生效） |
| POST | `/v1/cache/invalidate` | 物理清理 L0+L1 缓存（scope: all / prefix:xxx / 单 key）；异步执行，返回 202 Accepted |
| GET | `/v1/cache/invalidate/status` | 查询清理任务状态（all_in_progress + job snapshot） |
| GET | `/v1/cache/fingerprint` | 获取指纹版本和标准化配置 |
| PUT | `/v1/cache/fingerprint` | 更新指纹版本（升版本使旧键自然 miss，逻辑隔离，不扫 Redis） |
| GET | `/v1/cursor/models` | 获取 Cursor 模型别名表 |
| PUT | `/v1/cursor/models` | 热更新 Cursor 模型别名（`gpt-4o` → `deepseek-v4-pro` 等） |
| GET | `/v1/routing/backends` | 获取后端路由列表 |
| PUT | `/v1/routing/backends` | 热更新后端路由端点 |
| GET | `/v1/upstream/profiles` | 列出多厂商上游 Profile（脱敏） |
| PUT | `/v1/upstream/profiles/{id}` | 创建/更新 Profile（base_url、model、endpoints） |
| DELETE | `/v1/upstream/profiles/{id}` | 删除 Profile（保护 default/唯一项） |
| GET/PUT | `/v1/upstream/profiles/{id}/keys` | Profile 独立 API Key 池（`account_id` 可选，429 仅跨账号轮换） |
| PATCH | `/v1/upstream/profiles/{id}/keys/{key_id}` | 更新 Profile Key（仅 `enabled`） |
| POST | `/v1/upstream/profiles/{id}/test` | 探测 `GET {base_url}/v1/models` |
| GET | `/v1/domains/usage` | 获取域名月度用量计数（Admin 代理持久化） |
| PUT | `/v1/domains/usage` | 恢复域名月度用量计数（重启后从 PG 恢复） |

### Admin Dashboard

Admin Dashboard 分为后端（`crab-admin`，Axum HTTP 服务器）和前端（`crab-dashboard`，Leptos WASM CSR）。

后端提供丰富的管理 API（需 `x-admin-key` 认证）：
- 指标面板：QPS、TPS、缓存命中率、各层延迟
- 密钥管理：创建、吊销、配额设置
- 缓存配置：L0/L1 TTL 动态调整
- 语义缓存配置：相似度阈值调整
- 连接参数配置：TCP keepalive、H2 ping
- 上游配置：多 Profile（DeepSeek / MiMo 等）Base URL、Provider、API Key 池、端点列表（热更新至网关）
- 模型同步：按 Profile 从各厂商 `GET /v1/models` 同步模型目录（`admin-state.json` 带 `profile_id`）
- Trace 分析：请求分布、Zipf 参数估计、命中率预测
- 审计日志：管理操作记录（密钥 CRUD、策略变更、缓存清理等），存储于 PostgreSQL `audit_log` 表
- 请求日志查看

前端构建产物部署在 `crates/crab-dashboard/dist/` 目录，由 Admin 后端作为静态文件服务。

### API 密钥管理

- **Bootstrap Key**: 配置中的 `api_key` 自动作为引导密钥注册到 `RuntimeConfig.keys`
- **动态密钥**: 通过 Management API 创建/管理，存储在 `DashMap<String, StoredKey>`
- **密钥格式**: `sk-cc-{24 位十六进制}`（自动生成）
- **消费者标签**: 密钥的 `name` 字段作为消费者标签传递，用于 TTL 覆盖和指标维度
- **认证流程**: Bearer Token → DashMap 精确查找 → 未命中则检查 bootstrap_key

### Reasoning 处理管线

针对支持 reasoning/thinking 的 LLM 供应商（如 DeepSeek V4）的内容处理：

1. **请求预处理**: 注入 `thinking_mode`/`reasoning_effort` 参数，处理缺失 reasoning 恢复
2. **SSE Chunk 改写** (`rewrite_sse_chunk`): 拦截 reasoning 事件 → 提取内容 → 折叠到 assistant content 中
3. **Cursor 显示适配**: `CursorReasoningDisplayAdapter` 封装 `<thinking>`/`< tl;dr>` 折叠格式
4. **响应体改写** (`rewrite_response_body`): 非流式响应同样处理
5. **SQLite 缓存**: `ReasoningStore` 缓存已处理的 reasoning 内容，加速恢复

## 架构决策

### 为什么选择 Pingora？

1. **无锁连接池**: 两级连接池设计，90% 流量无锁处理
2. **共享内存模型**: 多线程共享后端连接，避免进程隔离的资源浪费
3. **原生 Rust**: 内存安全 + 零成本抽象
4. **生产验证**: Cloudflare 每日处理 1T+ 请求

### 为什么需要三级缓存？

1. **L0 (Moka)**: 极速拦截高频重复请求，零网络开销
2. **L1 (Redis)**: 跨实例共享缓存视图，提升全局命中率
3. **L2 (Qdrant)**: 捕获语义相似请求，覆盖"长尾"查询

### 为什么使用 Ketama 路由？

支持前缀缓存的 LLM 供应商（如 DeepSeek）要求请求必须路由到同一后端节点。Ketama 一致性哈希确保：
- 相同 conversation_id 的请求始终路由到同一节点
- 节点扩缩容时，仅影响 K/N 的请求（最小重分配）
- 保护已建立的 KV 缓存前缀

### 什么是 Request Coalescing？

当多个客户端同时发送完全相同的请求时（短时间内并发），Coalescing 机制确保只有第一个请求（Leader）发往上游，其余请求（Follower）等待 Leader 完成。Leader 完成时将响应写入缓存，Followers 直接从缓存读取。有效减少上游调用量和 Token 消耗。

## 性能目标

- **缓存命中率**: > 98%（综合 L0+L1+L2）
- **网关延迟**: P99 < 1ms（缓存命中）
- **缓存获取延迟**: L0 P99 < 100μs、L1 P99 < 5ms、L2 P99 < 50ms
- **首字延迟 (TTFT)**: 网关层附加延迟 < 5ms
- **吞吐量**: 单实例 > 10K QPS（缓存命中场景）

## 监控指标

Admin Dashboard Overview 通过 **`GET /api/admin/overview/core`** 每 10s 轮询（metrics 核心、health、L3 前缀、语义配置、运维 ops）；**`GET /api/admin/overview/timeseries`** 与 **`GET /api/admin/overview/trace`** 分别每 60s 加载时序与 24h Trace 摘要。展示 **5 分钟窗口**命中率（`hit_rate_5m`、`token_hit_rate_5m`）与 **进程累计**命中率，并区分 L0–L2 与 L3 口径。时序图来自 `crab-admin` 每 60s 采样的指标环；**1h** 窗口使用 5 分钟桶（最多 12 个），**24h** 使用 1 小时桶，**7d** 使用 1 天桶。完整 **`GET /api/admin/overview`** 仍可用。影子日志 Trace 页为近 24h 实测命中率。详见 [docs/OBSERVABILITY.md](docs/OBSERVABILITY.md)。

关键 Prometheus 指标（通过 `metrics_addr` 暴露）：

| 指标 | 类型 | 标签 | 描述 |
|------|------|------|------|
| `gateway_deepseek_input_tokens_total` | Counter | `{cache_status, model, consumer}` | 上游输入 Token（按命中/未命中分类） |
| `gateway_deepseek_output_tokens_total` | Counter | `{model, consumer}` | 上游输出 Token |
| `gateway_cache_requests_total` | Counter | `{tier, result}` | 缓存请求计数（按层级和结果） |
| `gateway_upstream_latency_seconds` | Histogram | `{model}` | 上游响应延迟 |
| `gateway_stream_first_token_latency_seconds` | Histogram | `{model}` | 流式首字延迟 |
| `gateway_cache_fetch_latency_seconds` | Histogram | `{tier}` | 缓存获取延迟 |
| `gateway_semantic_cache_requests_total` | Counter | `{status}` | 语义缓存请求状态 |
| `gateway_coalesced_requests_total` | Counter | - | 合并请求总数 |
| `gateway_cache_cost_saved_usd_total` | Counter | `{model, consumer, tier}` | 缓存节省成本估算 (USD) |
| `gateway_upstream_prompt_cache_tokens_total` | Counter | `{status, model, consumer}` | 上游提示缓存 Token 数 |
| `gateway_request_phase_latency_seconds` | Histogram | `{phase, pipeline, model}` | 请求生命周期各阶段耗时（如 `pipeline_select_done`、`upstream_body_sent`） |
| `gateway_prefix_index_warmup_total` | Counter | - | Prefix-aware L0 索引预热（未短路返回，请求继续上游） |

## 贡献指南

1. 遵循项目配置的 Rust 编码规范 Skills
2. 所有代码变更必须通过 `cargo clippy --workspace` 检查
3. 新功能需添加单元测试
4. 提交前运行 `cargo fmt`
5. Management API 变更需同步更新集成测试（`crates/crab-gateway/tests/`）
6. 配置项变更需同步更新 `config/gateway.example.toml`

## 参考资料

- [Pingora 官方文档](https://github.com/cloudflare/pingora)
- [DeepSeek V4 API 文档](https://api-docs.deepseek.com/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Moka Cache](https://github.com/moka-rs/moka)
- [Qdrant](https://qdrant.tech/documentation/)
- [Leptos](https://leptos.dev/)
- [项目详细方案文档](./Rust%20DeepSeek%20V4%20API%20网关方案.md)

## 许可证

MIT License