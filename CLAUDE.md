# CrabCache - DeepSeek V4 高性能 Rust API 网关

## 项目概述

CrabCache 是一个基于 Cloudflare Pingora 框架构建的高性能 Rust API 网关，专为 DeepSeek V4 大语言模型 API 设计。通过多级缓存架构（L0/L1/L2）和会话亲和性路由，实现极致的成本优化和低延迟响应。

### 核心特性

- **基于 Pingora 的高性能代理**：亚毫秒级延迟，无锁连接池
- **三级缓存防御体系**：
  - L0: Moka 进程内存缓存（P99 < 100ns）
  - L1: Redis 分布式精确缓存（P99 < 5ms）
  - L2: Qdrant 语义向量缓存（相似度阈值 0.95）
- **Ketama 一致性哈希路由**：最大化 DeepSeek V4 前缀缓存命中率
- **SSE 流式响应优化**：旁路拦截缓存，不阻塞实时下发
- **Prometheus 可观测性**：Token 成本追踪，延迟监控

## 技术栈

### 核心框架
- **Pingora 0.4**: Cloudflare 开源的高性能网络框架
- **Tokio**: 异步运行时
- **Rust 2024 Edition**

### 缓存系统
- **Moka**: 高并发内存缓存（TinyLFU 策略）
- **Redis**: 分布式精确缓存
- **bb8**: 异步连接池
- **Qdrant**: Rust 原生向量数据库

### AI/ML
- **ONNX Runtime (ort)**: 嵌入模型推理
- **all-MiniLM-L6-v2**: 语义向量化模型

### 可观测性
- **Prometheus**: 指标采集
- **tracing**: 结构化日志

## 项目结构

```
CrabCache/
├── Cargo.toml                  # Workspace 配置
├── config/                     # 运行时配置
│   └── gateway.example.toml
├── crates/
│   ├── crab-gateway/           # 主入口服务
│   ├── crab-proxy/             # ProxyHttp 实现
│   ├── crab-route/             # Ketama 路由
│   ├── crab-cache/             # L0/L1 精确缓存
│   ├── crab-semantic/          # L2 语义缓存
│   └── crab-metrics/           # Prometheus 指标
└── .trae/skills/               # Rust 编码规范 Skills
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
- **依赖服务**: Redis, Qdrant (可选)

### 构建与运行

```bash
# 开发模式
cargo build

# 生产构建
cargo build --release

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
- `upstream.deepseek_endpoints`: DeepSeek API 端点列表
- `cache`: L0/L1 缓存配置
- `semantic`: 语义缓存配置（ONNX 模型路径、Qdrant 地址）

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
# 从 Hugging Face 下载 all-MiniLM-L6-v2.onnx
```

模型文件约 80MB，不应提交到 Git。

#### 3. 内存分配器
生产环境使用 jemalloc 以提升高并发性能：
```rust
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

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

DeepSeek V4 的硬盘级前缀缓存要求请求必须路由到同一后端节点。Ketama 一致性哈希确保：
- 相同 conversation_id 的请求始终路由到同一节点
- 节点扩缩容时，仅影响 K/N 的请求（最小重分配）
- 保护已建立的 KV 缓存前缀

## 性能目标

- **缓存命中率**: > 98%（综合 L0+L1+L2）
- **网关延迟**: P99 < 1ms（缓存命中）
- **首字延迟 (TTFT)**: 与直连 DeepSeek 相比增加 < 5ms
- **吞吐量**: 单实例 > 10K QPS（缓存命中场景）

## 监控指标

关键 Prometheus 指标：
- `gateway_deepseek_input_tokens_total{cache_status="hit/miss"}`: Token 使用量
- `gateway_cache_requests_total{tier="L0/L1/L2", result="hit/miss"}`: 缓存效能
- `gateway_upstream_latency_seconds`: 上游响应延迟
- `gateway_stream_first_token_latency_seconds`: 首字延迟

## 贡献指南

1. 遵循项目配置的 Rust 编码规范 Skills
2. 所有代码变更必须通过 `cargo clippy --workspace` 检查
3. 新功能需添加单元测试
4. 提交前运行 `cargo fmt`

## 参考资料

- [Pingora 官方文档](https://github.com/cloudflare/pingora)
- [DeepSeek V4 API 文档](https://api-docs.deepseek.com/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [项目详细方案文档](./Rust%20DeepSeek%20V4%20API%20网关方案.md)

## 许可证

MIT License
