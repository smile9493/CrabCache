# CrabCache 供应商扩展计划 — 项目任务总纲

> 参考 OmniRoute（177 个供应商）将 CrabCache 从 5 个供应商扩展到 120+ 个内置供应商。
>
> **当前进度**：Phase 1（枚举/路由/管线）和 Phase 3（Dashboard UI）已完成；Phase 2（配置模板）、Phase 4（测试）、Phase 5（文档）待实施。

## 1. 背景与目标

### 1.1 现状

CrabCache **已扩展至 120+ 个内置上游供应商**（原计划 5 个 → 实际完成 120+）：

| 供应商 | 枚举值 | 特殊处理 |
|--------|--------|----------|
| DeepSeek | `Deepseek` | Reasoning/thinking 完整管线 |
| MiMo (小米) | `Mimo` | 会话级 key 绑定、429 冷却 |
| OpenAI | `Openai` | 纯透传 |
| Codex | `Codex` | OAuth + Responses API 协议转换 |
| Anthropic | `Anthropic` | 纯透传 |

### 1.2 目标

参考 `_externals/OmniRoute` 项目的供应商注册表（`open-sse/config/providerRegistry.ts` + `src/shared/constants/providers.ts`），将所有 **API Key 类型的 LLM 供应商** 内置到 CrabCache 中。

### 1.3 设计原则

- **零代码透传**：绝大多数供应商使用 OpenAI 兼容格式，走 `GenericRelay` pipeline，无需特殊处理
- **枚举驱动**：保持 CrabCache 现有的 `UpstreamProvider` 枚举 + `RequestPipeline` 架构
- **配置优先**：新增供应商主要通过扩展枚举 + 配置模板实现，不引入新框架
- **向后兼容**：保留 `Other` 兜底变体，未知供应商仍可工作

## 2. 架构影响分析

### 2.1 需要修改的文件

| 文件 | 改动类型 | 改动量 | 风险 |
|------|----------|--------|------|
| `crates/crab-pipeline/src/types.rs` | 枚举扩展 + match 分支 | ~300 行 | 低 |
| `crates/crab-pipeline/src/profile.rs` | 模型前缀→profile 映射 | ~60 行 | 低 |
| `crates/crab-pipeline/src/select.rs` | pipeline 选择 match 覆盖 | ~30 行 | 低 |
| `config/gateway.example.toml` | 供应商配置模板 | ~600 行 | 无 |
| `crates/crab-dashboard/src/pages/upstream.rs` | 前端供应商下拉列表 | ~100 行 | 低 |

### 2.2 不需要修改的文件

- `crates/crab-proxy/src/proxy.rs` — GenericRelay 已是通用透传
- `crates/crab-cache/` — 缓存层与供应商无关
- `crates/crab-route/` — 路由层与供应商无关
- `crates/crab-gateway/src/config.rs` — `UpstreamProfileConfig` 已是通用结构

### 2.3 编译影响

- 枚举变体从 6 个增加到 120+ 个
- `match` 语句变长，可用 `|` 合并同类分支
- 对编译时间影响可忽略（<1s）
- Clippy 可能触发 `clippy::match_same_arms`，已用 `|` 合并处理

## 3. 供应商分类（按 OmniRoute）

### 3.1 已有供应商（5 个，无需改动）

DeepSeek, MiMo, OpenAI, Codex, Anthropic

### 3.2 国际主流（15 个，优先添加）

Groq, xAI (Grok), Mistral, Gemini, Perplexity, Together AI, Fireworks AI, Cerebras, Cohere, NVIDIA NIM, Nebius AI, SiliconFlow, Hyperbolic, OpenRouter, Reka

### 3.3 云平台（7 个）

Azure OpenAI, Azure AI Foundry, Amazon Bedrock, Google Vertex AI, IBM watsonx, OCI Generative AI, SAP AI Hub

### 3.4 中国供应商（18 个）

阿里通义千问, 百度千帆, 智谱 GLM, Kimi (月之暗面), Minimax, Moonshot AI, 火山引擎, 豆包, 腾讯混元, 科大讯飞, 百川, 零一万物, 阶跃星辰, 360 AI, 商汤 SenseNova, 星火 SparkDesk, Coze, 百度 ERNIE

### 3.5 推理平台/聚合网关（60+ 个）

DeepInfra, Lambda AI, SambaNova, nScale, OVHcloud, Baseten, Databricks, Snowflake, W&B, AI21, GigaChat, Venice, Codestral, Upstage, Maritalk, Modal, HuggingFace, GitHub Models, Vercel AI Gateway, Meta Llama, v0, Morph, Featherless AI, LLM7, Lepton AI, Kluster AI, FriendliAI, LlamaGate, Heroku, Galadriel, DataRobot, Clarifai, Gitlawb, Inference.net, NanoGPT, Predibase, Bytez, AI/ML API, Novita AI, PiAPI, GoAPI, LaoZhang AI, GLHF, CablyAI, TheB.AI, FenayAI, Empower, Nous Research, Petals, Poe, GitLab Duo, Chutes.ai, Blackbox AI, BazaarLink, Completions.me, Enally AI, FreeTheAI, CrofAI, LongCat AI, Pollinations, Puter AI, UncloseAI, Replicate, Ollama Cloud, AgentRouter, Command Code, Astraflow, Phind, HuggingChat, Dify, PublicAI, Sapio, FreeAIAPIKey, BluesMinds, FreeModel.dev

## 4. 实施阶段

### Phase 1：核心枚举与路由（1-2 天）

- [x] 扩展 `UpstreamProvider` 枚举（120+ 变体）— `crates/crab-pipeline/src/types.rs`
- [x] 更新 `from_str()` / `as_str()` 方法 — 含多别名映射
- [x] 扩展 `model_prefix_to_profile()` 映射 — `crates/crab-pipeline/src/profile.rs`
- [x] 更新 `auto_pipeline_legacy()` match 分支 — `crates/crab-pipeline/src/select.rs`
- [ ] 添加单元测试（供应商 roundtrip、profile 路由覆盖）

### Phase 2：配置模板（0.5 天）

- [ ] 在 `gateway.example.toml` 中添加所有供应商的 profile 配置示例
- [ ] 验证配置文件可被正确解析

### Phase 3：Dashboard UI（1 天）

- [x] 更新上游 Profile 页面的供应商预设模板 — `crates/crab-dashboard/src/pages/upstream.rs`（~58 个 `PresetTemplate`）
- [x] 更新模型同步逻辑（如有）

### Phase 4：测试与验证（1 天）

- [ ] `cargo clippy --workspace` 无新警告
- [ ] `cargo test --workspace` 全部通过
- [ ] 配置 3-5 个真实供应商进行端到端验证
- [ ] 更新集成测试

### Phase 5：文档更新（0.5 天）

- [ ] 更新 `CLAUDE.md` 供应商相关描述
- [ ] 更新 `config/gateway.example.toml` 注释
- [ ] 创建供应商配置指南

## 5. 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 枚举变体过多导致编译变慢 | 低 | 低 | 实测 <1s 影响可忽略 |
| match 分支遗漏导致编译错误 | 中 | 低 | 编译器会强制检查穷尽性 |
| 某些供应商 API 格式不兼容 | 中 | 中 | 保留 `Other` 兜底 + 日志告警 |
| 模型前缀冲突 | 低 | 中 | 优先级排序 + 精确前缀匹配 |
| 前端下拉列表过长 | 中 | 低 | 分组显示 + 搜索过滤 |

## 6. 参考文档

| 文档 | 路径 | 说明 |
|------|------|------|
| OmniRoute 供应商参考 | [`docs/OMNIRROUTE_PROVIDERS_REFERENCE.md`](OMNIRROUTE_PROVIDERS_REFERENCE.md) | 从 OmniRoute 扫描的完整供应商清单 |
| 任务清单 | [`docs/tasks/`](tasks/) | 分解的子任务与进度跟踪 |
| 任务总纲 | 本文档 | 项目全局视图 |
| Pipeline 架构 | [`crates/crab-pipeline/src/types.rs`](../crates/crab-pipeline/src/types.rs) | UpstreamProvider / RequestPipeline 定义 |
| Profile 路由 | [`crates/crab-pipeline/src/profile.rs`](../crates/crab-pipeline/src/profile.rs) | 模型名→profile 映射逻辑 |
| Pipeline 选择 | [`crates/crab-pipeline/src/select.rs`](../crates/crab-pipeline/src/select.rs) | 请求→pipeline 选择逻辑 |
| 配置模板 | [`config/gateway.example.toml`](../config/gateway.example.toml) | 运行时配置示例 |
| OmniRoute 源码 | [`_externals/OmniRoute/`](../_externals/OmniRoute/) | 参考实现（TypeScript） |
