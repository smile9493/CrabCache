# CrabCache 分阶段任务规划

> 当前状态：Workspace `Cargo.toml` + 6 个 crate 的 `Cargo.toml` + `.gitignore` + `config/gateway.example.toml` 已创建。
> 接下来需要按依赖顺序逐 crate 填充源码。

---

## Phase 1 — 叶子 Crate（无内部依赖）

### Task 1.1 `crab-metrics` 源码

- [x] `crates/crab-metrics/Cargo.toml`
- [ ] `crates/crab-metrics/src/lib.rs` — 模块入口，re-export `registry`
- [ ] `crates/crab-metrics/src/registry.rs` — 全局指标注册中心

**`registry.rs` 要实现的内容：**

```rust
pub struct GatewayMetrics {
    // LLM Token 财务指标
    pub input_tokens: IntCounterVec,     // labels: [cache_status, model, consumer]
    pub output_tokens: IntCounterVec,    // labels: [model, consumer]

    // 多层缓存效能
    pub cache_requests: IntCounterVec,   // labels: [tier, result]
    // tier ∈ {"L0_moka", "L1_redis", "L2_semantic"}
    // result ∈ {"hit", "miss"}

    // QoS 延迟直方图
    pub upstream_latency: HistogramVec,       // labels: [model]
    pub ttft: HistogramVec,                   // Time To First Token, labels: [model]
    pub cache_fetch_latency: HistogramVec,    // labels: [tier]

    // 语义缓存
    pub semantic_requests: IntCounterVec,     // labels: [status]

    // 请求合并
    pub coalesced_requests: IntCounter,
}
```

- 使用 `once_cell::sync::Lazy<GatewayMetrics>` 提供全局单例
- 提供便捷方法：`record_cache_hit(tier, model, consumer)`, `record_upstream_usage(usage, model, consumer)`, `record_latency(kind, duration)`
- 定义 `pub enum CacheTier { L0Moka, L1Redis, L2Semantic, Miss }` 和 `pub enum LatencyKind { Upstream, TTFT, CacheFetch }`

**验收标准：** `cargo test -p crab-metrics` 通过，指标能正确注册到默认 `prometheus::Registry`。

---

### Task 1.2 `crab-route` 源码

- [x] `crates/crab-route/Cargo.toml`
- [ ] `crates/crab-route/src/lib.rs` — re-export
- [ ] `crates/crab-route/src/ring.rs` — Ketama 哈希环封装
- [ ] `crates/crab-route/src/affinity.rs` — 亲和性键提取

**`ring.rs` 核心：**

```rust
pub struct Backend {
    pub name: String,
    pub addr: SocketAddr,  // 用于构造 HttpPeer
    pub weight: u32,
    pub tls_sni: String,
}

pub struct AffinityRouter {
    continuum: pingora_ketama::Continuum,
    backends: Vec<Arc<Backend>>,
}

impl AffinityRouter {
    pub fn new(backends: &[Backend]) -> Self;
    pub fn select(&self, key: &[u8]) -> &Backend;  // 返回哈希环命中的后端
    pub fn update(&mut self, backends: &[Backend]); // 热更新
}
```

**`affinity.rs` 核心：**

```rust
/// 从请求头提取亲和性键，降级顺序：
/// x-conversation-id → x-user-id → client source IP
pub fn extract_affinity_key(headers: &http::HeaderMap, client_ip: &str) -> String;
```

**验收标准：**
- 单测：3 个后端，相同 key 始终路由到同一 backend
- 单测：新增 1 个后端后，漂移率 < 40%

---

## Phase 2 — 缓存子系统

### Task 2.1 `crab-cache` 源码

- [x] `crates/crab-cache/Cargo.toml`
- [ ] `crates/crab-cache/src/lib.rs`
- [ ] `crates/crab-cache/src/types.rs` — 共享类型定义
- [ ] `crates/crab-cache/src/key.rs` — CacheKey 生成
- [ ] `crates/crab-cache/src/tiered.rs` — 多级缓存协调器
- [ ] `crates/crab-cache/src/coalescing.rs` — 请求合并防击穿

**`types.rs`：**

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub response_body: Vec<u8>,  // 完整 SSE 响应或 JSON
    pub model: String,
    pub usage: UsageInfo,
    pub created_at: u64,         // unix timestamp
    pub ttl_secs: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub prompt_cache_hit_tokens: u64,
    pub prompt_cache_miss_tokens: u64,
}

pub struct TtlConfig {
    pub default_ttl_secs: u64,
    pub model_overrides: HashMap<String, u64>,
    pub consumer_overrides: HashMap<String, u64>,
}

impl TtlConfig {
    pub fn resolve(&self, model: &str, consumer: Option<&str>) -> u64;
    // consumer_override > model_override > default
}
```

**`key.rs`：**

```rust
/// 规范化请求体后 SHA-256 哈希生成 cache key
/// 规范化：排序 JSON 字段、去除 stream/temperature 等不影响结果的字段
pub fn generate_cache_key(request_body: &[u8]) -> Result<String>;
```

**`tiered.rs`：**

```rust
pub struct TieredCache {
    l0: moka::future::Cache<String, CacheEntry>,
    l1_pool: bb8::Pool<bb8_redis::RedisConnectionManager>,
    ttl_config: TtlConfig,
    coalescer: RequestCoalescer,
}

impl TieredCache {
    pub async fn new(cache_config: &CacheConfig) -> Result<Self>;
    pub async fn get(&self, key: &str) -> Option<(CacheEntry, CacheTier)>;
    pub async fn put(&self, key: &str, entry: CacheEntry, model: &str, consumer: Option<&str>) -> Result<()>;
}
```

**`coalescing.rs`：**

```rust
pub struct RequestCoalescer {
    inflight: DashMap<String, Arc<tokio::sync::Notify>>,
}

impl RequestCoalescer {
    /// 如果 key 已在飞行中，等待并返回 true（表示有人已在获取）
    /// 如果 key 不存在，注册并返回 false（调用者是"先锋"）
    pub async fn acquire(&self, key: &str) -> CoalesceGuard;
    pub fn release(&self, key: &str);  // 先锋完成后唤醒所有等待者
}
```

**验收标准：**
- `key.rs`：相同语义请求体（字段顺序不同）生成相同 key
- `tiered.rs`：mock Redis 测试 L0 命中 → 不查 L1；L0 miss + L1 hit → 回填 L0
- `coalescing.rs`：并发 100 个相同 key 请求，只有 1 个穿透

---

## Phase 3 — 语义缓存子系统

### Task 3.1 `crab-semantic` 源码

- [x] `crates/crab-semantic/Cargo.toml`
- [ ] `crates/crab-semantic/src/lib.rs`
- [ ] `crates/crab-semantic/src/embedder.rs` — ort ONNX 推理封装
- [ ] `crates/crab-semantic/src/store.rs` — Qdrant 向量存储
- [ ] `crates/crab-semantic/src/cache.rs` — 语义缓存协调

**`embedder.rs`：**

```rust
pub struct Embedder {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
}

impl Embedder {
    pub fn load(model_path: &str) -> Result<Self>;

    /// 关键：使用 tokio::task::spawn_blocking 卸载到阻塞线程池
    /// 避免阻塞 Pingora 异步事件循环
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
```

**`store.rs`：**

```rust
pub struct VectorStore {
    client: qdrant_client::Qdrant,
    collection: String,
}

impl VectorStore {
    pub async fn new(url: &str, collection: &str, vector_size: u64) -> Result<Self>;
    pub async fn ensure_collection(&self) -> Result<()>;  // 建表
    pub async fn search(&self, vector: &[f32], threshold: f32) -> Option<CacheEntry>;
    pub async fn upsert(&self, id: &str, vector: &[f32], entry: &CacheEntry, ttl_secs: u64) -> Result<()>;
}
```

**`cache.rs`：**

```rust
pub struct SemanticCache {
    embedder: Arc<Embedder>,
    store: VectorStore,
    threshold: f32,
    ttl_secs: u64,
}

impl SemanticCache {
    pub async fn new(config: &SemanticConfig) -> Result<Self>;
    pub async fn search(&self, query_text: &str) -> Option<CacheEntry>;
    pub async fn insert(&self, query_text: &str, entry: &CacheEntry) -> Result<()>;
}
```

**验收标准：**
- `embedder.rs`：加载 ONNX 模型，输入 "hello world" 返回 384 维向量
- `store.rs`：插入后能按相似度检索回来
- `cache.rs`："解释这段代码" 与 "这段代码是什么意思" 相似度 > 0.9

---

## Phase 4 — 代理核心

### Task 4.1 `crab-proxy` 源码

- [x] `crates/crab-proxy/Cargo.toml`
- [ ] `crates/crab-proxy/src/lib.rs`
- [ ] `crates/crab-proxy/src/context.rs` — 请求级上下文
- [ ] `crates/crab-proxy/src/sse.rs` — SSE 流式响应解析
- [ ] `crates/crab-proxy/src/proxy.rs` — ProxyHttp trait 实现

**`context.rs`：**

```rust
pub struct RequestContext {
    pub request_id: String,              // UUID v4
    pub conversation_id: Option<String>, // from x-conversation-id
    pub user_id: Option<String>,         // from x-user-id
    pub consumer: Option<String>,        // from x-consumer-id
    pub cache_key: Option<String>,       // SHA-256 of normalized body
    pub request_body: Vec<u8>,           // 完整请求体（用于缓存 key 计算）
    pub response_chunks: Vec<bytes::Bytes>,  // SSE 旁路收集
    pub cache_hit_tier: Option<CacheTier>,
    pub start_time: std::time::Instant,
    pub first_token_time: Option<std::time::Instant>,
    pub model: String,
}
```

**`sse.rs`：**

```rust
/// 从完成的 SSE 响应块中提取最终的 usage 信息
/// DeepSeek SSE 最后一个 data 块包含 usage 字段：
/// data: {"usage":{"prompt_tokens":...,"completion_tokens":...,"prompt_cache_hit_tokens":...,"prompt_cache_miss_tokens":...}}
pub fn extract_usage_from_sse(chunks: &[Bytes]) -> Option<UsageInfo>;

/// 将收集到的所有 SSE chunks 拼接为完整响应体（用于缓存存储）
pub fn reassemble_sse_body(chunks: &[Bytes]) -> Vec<u8>;
```

**`proxy.rs` — ProxyHttp 生命周期 5 阶段：**

```rust
pub struct CrabProxy {
    pub router: Arc<AffinityRouter>,
    pub cache: Arc<TieredCache>,
    pub semantic: Option<Arc<SemanticCache>>,  // 可选
    pub metrics: &'static GatewayMetrics,
    pub api_keys: Vec<ApiKeyEntry>,
}

impl ProxyHttp for CrabProxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> Self::CTX;

    // Phase 1: 读取请求体，计算 cache_key，查 L0 → L1 → L2
    //          命中则构造响应直接返回 (session.respond(...))
    async fn request_filter(&self, session, ctx) -> Result<bool>;

    // Phase 2: Ketama 路由选择后端
    async fn upstream_peer(&self, session, ctx) -> Result<Box<HttpPeer>>;

    // Phase 3: 注入 Authorization header (API Key)
    async fn upstream_request_filter(&self, session, upstream_req, ctx) -> Result<()>;

    // Phase 4: 旁路克隆 SSE chunks，记录 first_token_time
    fn response_body_filter(&self, session, body, eof, ctx) -> Result<Option<Duration>>;

    // Phase 5: 解析 usage → 回填缓存 → 上报 metrics
    async fn logging(&self, session, error, ctx);
}
```

**关键实现细节：**
- `request_filter` 中缓存命中时，需要手动构造 HTTP 200 响应 + 正确的 Content-Type（`text/event-stream` 或 `application/json`），通过 `session.write_response_header/body` 直接下发
- `response_body_filter` 中 **只克隆不消费** body，确保 Pingora 引擎继续向客户端推送
- `upstream_request_filter` 中从 `api_keys` 按权重轮询选择 Key 注入 `Authorization: Bearer sk-xxx`
- `logging` 阶段如果请求是 cache miss，调用 `cache.put()` 和 `semantic.insert()` 回填

**验收标准：**
- 编译通过，ProxyHttp trait 的 5 个方法签名正确
- `sse.rs`：给定 mock SSE chunks 能正确提取 usage

---

## Phase 5 — 主入口

### Task 5.1 `crab-gateway` 源码

- [x] `crates/crab-gateway/Cargo.toml`
- [ ] `crates/crab-gateway/src/main.rs` — 入口
- [ ] `crates/crab-gateway/src/config.rs` — 配置反序列化

**`config.rs`：**

```rust
#[derive(Deserialize)]
pub struct GatewayConfig {
    pub gateway: GatewaySection,
    pub upstream: UpstreamConfig,
    pub cache: CacheConfig,
    pub semantic: SemanticConfig,
}

#[derive(Deserialize)]
pub struct GatewaySection {
    pub listen_addr: String,
    pub metrics_addr: String,
}

#[derive(Deserialize)]
pub struct UpstreamConfig {
    pub api_base: String,
    pub model: String,
    pub api_keys: Vec<ApiKeyEntry>,
}

#[derive(Deserialize, Clone)]
pub struct ApiKeyEntry {
    pub key: String,
    pub weight: u32,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct CacheConfig {
    pub default_ttl_secs: u64,
    pub l0_max_entries: u64,
    pub l0_max_memory_mb: u64,
    pub redis_url: String,
    pub redis_pool_size: u32,
    pub model_overrides: Option<HashMap<String, u64>>,
    pub consumer_overrides: Option<HashMap<String, u64>>,
}

#[derive(Deserialize)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub onnx_model_path: String,
    pub qdrant_url: String,
    pub collection_name: String,
    pub similarity_threshold: f32,
    pub ttl_secs: u64,
}

pub fn load_config(path: &str) -> Result<GatewayConfig>;
```

**`main.rs` 流程：**

```rust
// 1. 注入 jemalloc（仅 Linux target）
// #[cfg(target_os = "linux")]
// #[global_allocator]
// static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn main() -> Result<()> {
    // 2. 初始化 tracing
    tracing_subscriber::fmt().with_env_filter("info").init();

    // 3. 加载配置
    let config = config::load_config("config/gateway.toml")?;

    // 4. 构建子系统
    let metrics = crab_metrics::global_metrics();
    let router = Arc::new(AffinityRouter::new(&build_backends(&config.upstream)));
    let cache = block_on(TieredCache::new(&config.cache))?;
    let semantic = if config.semantic.enabled {
        Some(Arc::new(block_on(SemanticCache::new(&config.semantic))?))
    } else { None };

    // 5. 组装 CrabProxy
    let proxy = CrabProxy { router, cache, semantic, metrics, api_keys: config.upstream.api_keys };

    // 6. 启动 Pingora Server
    let mut server = pingora::server::Server::new(None)?;
    server.bootstrap();
    let mut proxy_service = pingora::proxy::http_proxy_service(
        &server.configuration, proxy,
    );
    proxy_service.add_tcp(&config.gateway.listen_addr);
    server.add_service(proxy_service);

    // 7. 挂载 Prometheus metrics endpoint
    let mut metrics_service = pingora::services::listening::Service::prometheus_http_service();
    metrics_service.add_tcp(&config.gateway.metrics_addr);
    server.add_service(metrics_service);

    // 8. 运行
    server.run_forever();
}
```

**验收标准：**
- `config.rs`：能正确反序列化 `gateway.example.toml`
- `main.rs`：编译通过，启动后能监听端口（无后端也能运行）

---

## Phase 6 — Docker & 部署

### Task 6.1 容器化

- [ ] `docker/Dockerfile` — 多阶段构建
- [ ] `docker/docker-compose.yml` — 全栈编排

**Dockerfile 设计：**
- Stage 1: `rust:1.87-slim-bookworm` 编译
  - `cargo build --release -p crab-gateway`
- Stage 2: `debian:bookworm-slim` 运行时
  - 仅复制二进制 + 配置 + 模型文件
  - 暴露 8080 (proxy) 和 9090 (metrics)

**docker-compose.yml 服务清单：**

| 服务 | 镜像 | 端口 | 资源限制 |
|------|------|------|----------|
| crab-gateway | 自构建 | 8080, 9090 | 4C / 4GB |
| redis | redis:7-alpine | 6379 | 1GB max |
| qdrant | qdrant/qdrant:latest | 6333, 6334 | 2GB |
| prometheus | prom/prometheus:latest | 9091 | - |
| grafana | grafana/grafana:latest | 3000 | - |

- Prometheus 配置 scrape `crab-gateway:9090/metrics`
- 所有数据卷挂载到 `docker/data/`

---

## Phase 7 — 验证

### Task 7.1 编译验证
- [ ] `cargo check --workspace` 通过
- [ ] `cargo clippy --workspace` 无警告
- [ ] `cargo test --workspace` 单元测试全绿

### Task 7.2 集成冒烟测试
- [ ] `docker-compose up` 全栈启动
- [ ] `curl -X POST http://localhost:8080/v1/chat/completions` 发送测试请求
- [ ] 验证首次请求成功 → cache miss → L1 回填
- [ ] 验证相同请求再次发送 → cache hit (L0)
- [ ] 验证 `curl http://localhost:9090/metrics` 能看到指标

---

## 执行顺序总结

```
Phase 1.1  crab-metrics  ─┐
Phase 1.2  crab-route    ─┤
                          ├─► Phase 2.1  crab-cache
                          │
                          └─► Phase 3.1  crab-semantic
                                          │
Phase 2.1 + 3.1 ─────────────► Phase 4.1  crab-proxy
                                           │
                               Phase 5.1  crab-gateway
                                           │
                               Phase 6.1  Docker
                                           │
                               Phase 7    验证
```

预计工作量：每个 Phase 约 1 轮对话即可完成。
