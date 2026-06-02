# 任务 07: 文档更新

> **Phase**: 5 | **优先级**: P2 | **状态**: 待开始
> **预计工作量**: 30 分钟 | **风险**: 无

## 目标

更新项目文档，反映新增的供应商支持。

## 修改文件

- `CLAUDE.md` — 项目概述中的供应商描述
- `README.md` — 如果有供应商相关描述
- `config/gateway.example.toml` — 注释更新

## 详细步骤

### Step 1: 更新 CLAUDE.md

在项目概述部分更新供应商描述：

```markdown
### 支持的供应商

CrabCache 内置 120+ 个 LLM 供应商，覆盖国际主流、中国本土、云平台和推理聚合网关：

- **国际主流**: OpenAI, Anthropic, DeepSeek, Groq, xAI (Grok), Mistral, Google Gemini, Perplexity, Together AI, Fireworks AI, Cerebras, Cohere, NVIDIA NIM
- **中国供应商**: 阿里通义千问, 百度千帆, 智谱 GLM, Kimi, Minimax, MiMo, 腾讯混元, 科大讯飞, 百川, 零一万物, 阶跃星辰, 豆包, 火山引擎
- **云平台**: Azure OpenAI, Amazon Bedrock, Google Vertex AI, IBM watsonx, OCI, SAP
- **聚合网关**: OpenRouter, DeepInfra, SiliconFlow, HuggingFace, GitHub Models
```

### Step 2: 更新 gateway.example.toml 注释

在配置文件头部添加供应商列表注释：

```toml
# CrabCache 支持 120+ 个 LLM 供应商，包括：
# DeepSeek, OpenAI, Anthropic, MiMo, Groq, xAI, Mistral, Gemini,
# Perplexity, Together, Fireworks, Cerebras, Cohere, NVIDIA, OpenRouter,
# 阿里通义, 百度千帆, 智谱GLM, Kimi, Minimax, 腾讯混元, 科大讯飞,
# 百川, 零一万物, 阶跃星辰, 豆包, Azure, Bedrock, Vertex AI, ...
#
# 完整列表见 docs/OMNIRROUTE_PROVIDERS_REFERENCE.md
```

### Step 3: 创建供应商快速入门指南（可选）

在 `docs/` 中创建 `PROVIDER_QUICKSTART.md`，指导用户如何快速接入新供应商。

## 验收标准

- [ ] `CLAUDE.md` 中的供应商描述已更新
- [ ] `gateway.example.toml` 头部注释已更新
- [ ] 文档中的供应商数量与实际枚举变体数一致

## 依赖

- 依赖任务 01-04 全部完成

## 注意事项

- `CLAUDE.md` 是项目的核心文档，修改需谨慎
- 保持文档风格与现有内容一致
- 供应商数量以 `UpstreamProvider` 枚举变体数为准（不含 `Other`）
