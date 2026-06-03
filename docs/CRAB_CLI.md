# CrabCache CLI（crab-cli）

独立的运维诊断 CLI 工具，用 Rust 编写，替代 `scripts/analyze_gateway_logs.py` 等 Python 脚本。

**设计原则**：不依赖 `crab-proxy`/Pingora，仅引入 `clap` + `serde` + `regex`，编译快、体积小。在**本机**构建，在**本机**运行；远程数据通过 SSH 自动拉取。

---

## 构建

```bash
# 调试版
cargo build -p crab-cli

# 发布版
cargo build --release -p crab-cli

# 二进制路径
target/release/crab-cli
```

---

## 子命令速查

```
crab-cli
├── report  --target <name> [--tail N] [--log-tail N] [--compare-tail N] [--since ISO]
├── trace
│   ├── analyze   <file|-> [--target <name>] [--tail N]
│   ├── keys      <file|-> [--target <name>] [--tail N]
│   ├── compare   <file|-> [--target <name>] (--compare-tail N | --split-at N) [--tail N]
│   └── cache     <file|-> [--target <name>] [--tail N]
├── health
│   ├── logs      <file|-> [--target <name>] [--tail N] [--since ISO]
│   └── metrics   <file|-> [--target <name>]
└── density       <file|-> [--target <name>] [--tail N]
```

所有子命令都接受 **文件路径**（含 `-` 读 stdin）或 **`--target wuming|crabcache-deploy`**（通过 SSH 远程抓取）。`--tail N` 限制读取行数（默认 500）。

---

## 子命令详解

### `report` — 一站式全量报告

串联 **trace 延迟 + Key 分布 + 缓存拟合 + 内容密度 + Docker 日志 + Prometheus**，适合热更新后快速验收。

```bash
# 远程 wuming，最近 500 行 + 对比最近 100 行前后
crab-cli report --target wuming --tail 500 --compare-tail 100

# 限制 Docker 日志时间范围
crab-cli report --target wuming --since 2026-06-03T10:00:00Z
```

输出包含：
- before/after 对比（`--compare-tail` 时）
- 延迟分解（prefill / upstream / e2e / ttft / gap）
- 运行时信号（defer、passthrough、session、affinity）
- 按 pipeline / body size / client_kind / passthrough 分桶
- top 5 最慢 prefill
- Key 分布 + 429 检测
- 缓存 Zipf 拟合
- 内容密度
- Docker 日志健康扫描
- Prometheus 指标摘要

### `trace analyze` — 延迟分解

替代 `scripts/analyze_downstream_latency.py`。

```bash
crab-cli trace analyze /path/to/trace.jsonl --tail 500
crab-cli trace analyze - --tail 200                     # stdin
crab-cli trace analyze --target wuming --tail 1000      # 远程
```

输出：prefill / upstream / e2e / ttft / gap（min/p50/p90/max/mean），按 pipeline、body size、client_kind、defer/passthrough 分桶，top 5 最慢请求。

### `trace keys` — Key 分布分析

```bash
crab-cli trace keys /path/to/trace.jsonl --tail 1000
```

输出：各 `upstream_key_id` 的请求数、成功/429/失败、均匀度 CV、429 样本、`upstream_key_exhausted` 计数。

### `trace compare` — before/after 窗口对比

```bash
# tail 窗口：最后 100 行 vs 前 100 行
crab-cli trace compare file.jsonl --compare-tail 100

# 行索引分界：第 452 行前后
crab-cli trace compare file.jsonl --split-at 452
```

输出：prefill_p50 / upstream_p50 变化、cache_hit_rate 变化、Key 分布偏移。

### `trace cache` — Zipf 拟合

```bash
crab-cli trace cache file.jsonl --tail 5000
```

输出：total/unique requests、repeat_ratio、semantic_cluster_ratio、Zipf α、conversation_ratio、cache_hit_rate、estimated achievable hit rate。

### `health logs` — Docker 日志健康扫描

替代 `scripts/analyze_gateway_logs.py --docker-logs`。

```bash
crab-cli health logs --target wuming --tail 1000
crab-cli health logs --target wuming --since 2026-06-03T10:00:00Z
crab-cli health logs /path/to/docker-logs.txt
ssh wuming 'docker logs crabcache-gateway-1 --tail 1000 2>&1' | crab-cli health logs -
```

扫描分类：JSON parse_fail（含 streaming_defer=true）、Tokio panic、Redis bb8 timeout、ConnectionClosed、upstream 400、其他 ERROR。

### `health metrics` — Prometheus 指标摘要

```bash
crab-cli health metrics --target wuming
crab-cli health metrics /path/to/metrics.txt
ssh wuming 'curl -sf http://127.0.0.1:9090/metrics' | crab-cli health metrics -
```

输出：streaming defer 计数、passthrough 计数、key_binding 事件、rejected 请求（按 reason）、upstream key pool 状态、phase latency（按 model × phase 平均值）、upstream latency 按 model 汇总。

### `density` — 内容密度

```bash
crab-cli density --target wuming --tail 1000
crab-cli density file.jsonl
```

输出：reasoning_stripped / upstream_outbound 比率、按 pipeline 分桶（n、hit%、strip%、upstream→client 比率）。

---

## 与 Python 脚本的对应关系

| Python 脚本 | crab-cli 替代 | 备注 |
|-------------|--------------|------|
| `scripts/analyze_gateway_logs.py --trace` | `crab-cli trace analyze` | 延迟分解 |
| `scripts/analyze_gateway_logs.py --docker-logs` | `crab-cli health logs` | Docker 日志健康 |
| `scripts/analyze_gateway_logs.py --metrics` | `crab-cli health metrics` | Prometheus |
| `scripts/analyze_gateway_logs.py --target ... --compare-tail N` | `crab-cli report --compare-tail N` | 一站式 |
| `scripts/analyze_downstream_latency.py` | `crab-cli trace analyze` | 同上 |
| `scripts/analyze_trace.py` | `crab-cli trace cache` | Zipf 拟合 |
| `scripts/content_density_analyzer.py --trace` | `crab-cli density` | 内容密度 |
| `scripts/collect_trace.sh` | `--target` 自动 SSH | 内置于各子命令 |

---

## 远程目标（`--target`）

| 目标 | SSH 别名 | 容器 | 用途 |
|------|---------|------|------|
| `wuming` | `wuming` | `crabcache-gateway-1` | 公网生产 |
| `crabcache-deploy` | `crabcache-deploy` | `crabcache-gateway-1` | 内网测试 |

SSH 别名需已在 `~/.ssh/config` 中配置，详见 [AGENTS.md](../AGENTS.md)。

---

## Trace JSONL 字段参考

`crab-cli` 读取的字段来自 `SanitizedLogEntry`（定义在 `crates/crab-proxy/src/trace_logger.rs`），CLI 在 `crates/crab-cli/src/trace_entry.rs` 本地定义了相同字段子集（带 `#[serde(default)]` 保证向前兼容）。

核心字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `timestamp_ms` | u64 | 请求时间戳 |
| `request_hash` | String | 请求体 SHA256 前缀 |
| `content_length` | usize | 请求体字节数 |
| `model` | String | 模型名 |
| `cache_hit` | bool | 是否命中缓存 |
| `latency_ms` | f64 | 端到端延迟 |
| `upstream_latency_ms` | Option | 上游响应延迟 |
| `prefill_ms` | Option | 请求开始→上游响应头（MiMo SLO） |
| `ttft_ms` | Option | 响应头→首个 SSE chunk |
| `pipeline` | Option | 请求管线标识 |
| `client_kind` | Option | 客户端类型（cursor/codex/generic） |
| `upstream_key_id` | Option | 使用的上游 API Key ID |
| `status_code` | Option | 上游 HTTP 状态码 |
| `upstream_result` | Option | 上游结果分类 |
| `reasoning_stripped_bytes` | Option | 被剥离的 reasoning 内容字节数 |
| `upstream_outbound_bytes` | Option | 发往上游的实际字节数 |
