# 数据面 P3 长期演进（实验性）

本文档描述 [数据面优化.md](../数据面优化.md) 中 P3 项的设计约束、回滚策略与前置开关。实现为独立 feature flag，默认关闭。

**P0–P2 已交付项**见 [DATA_PLANE.md](./DATA_PLANE.md)，勿与本文件混淆。

## P3-1：差分缓存（Delta Cache）

### 目标

长会话（60+ messages）仅缓存相对上一轮的增量响应，降低 L0/L1 单条 entry 体积（目标 ~50KB vs ~2MB）。

### 键与值（草案）

```
Cache Key: SHA-256(messages[0..n-1]) + "Δ" + SHA-256(message[n])
Cache Value: {
  base_key: SHA-256(messages[0..n-1]),
  delta_response: "...",
  usage: { ... }
}
```

### 回滚

- `features.delta_cache = false`（默认）：完全不读写差分 entry，仅使用现有完整响应缓存。
- 升 `fingerprint_version` 可使旧差分键自然 miss。

### 风险

- 重建完整响应需 base entry 仍存在；base 过期则必须 miss 并回源。
- 与语义缓存、prefix cache 的交互需单独定义优先级。

---

## P3-2：WASM Filter 插件化（PoC）

### 目标

将 body/SSE 变换从编译期 pipeline 抽离为可加载 WASM 模块，便于新供应商适配无需重编译网关。

### 架构草案

```
crab-proxy (host)
  ├── builtin: PassthroughPipeline / ReasoningRewritePipeline (现有)
  └── wasm_filters/   # 可选目录，features.wasm_filters = true
        ├── anthropic_adapter.wasm
        └── custom_sse_filter.wasm
```

### 回滚

- `features.wasm_filters = false`：仅使用 Rust builtin pipeline。

### 风险

- 需 WASM 运行时依赖与沙箱（内存上限、超时）。
- 热路径跨语言边界延迟需 benchmark。

---

## P3-3：io_uring 与多模态

### io_uring

- **依赖**：Pingora 上游支持 io_uring 后端（当前为 epoll/kqueue）。
- **开关**：`features.io_uring_backend`（预留，默认 false）。
- **收益（预期）**：sendfile 零拷贝、批量 syscall、异步日志 I/O。

### 多模态

| 挑战 | 当前 | 方向 |
|------|------|------|
| 大 body（10+ MB） | `max_request_body_bytes = 4MB` | 提升限制 + 流式转发 |
| 缓存键 | 全量 body SHA-256 | metadata hash + content hash 分层 |
| 语义缓存 | all-MiniLM-L6-v2 文本 | 多模态嵌入（CLIP/SigLIP） |
| SSE 二进制 | 文本解析 | 二进制感知 SSE |

---

## 相关配置

见 `config/gateway.example.toml` 中 `[features]` 段：

- `affinity_prompt_cache_feedback` — L3 上游 prompt cache 粘性路由（P2-3）
- `delta_cache` — 差分缓存实验（P3-1）
- `io_uring_backend` — 预留（P3-3）
- `wasm_filters` — WASM 插件实验（P3-2）
