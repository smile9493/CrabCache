# 任务 04: 配置模板扩展

> **Phase**: 2 | **优先级**: P1 | **状态**: 待开始
> **预计工作量**: 2 小时 | **风险**: 无

## 目标

在 `config/gateway.example.toml` 中添加所有新增供应商的配置示例，方便用户快速接入。

## 修改文件

- `config/gateway.example.toml`

## 详细步骤

### Step 1: 添加国际主流供应商配置

在现有的 `[[upstream.profiles]]` 段后追加：

```toml
# ═══════════════════════════════════════════════════════════
# 国际主流供应商
# ═══════════════════════════════════════════════════════════

# ── Groq（超低延迟推理）──
[[upstream.profiles]]
id = "groq"
provider = "groq"
base_url = "https://api.groq.com/openai/v1"
endpoints = ["api.groq.com:443"]
model = "llama-3.3-70b-versatile"
keys = ["gsk_your_groq_key"]

# ── xAI (Grok) ──
[[upstream.profiles]]
id = "xai"
provider = "xai"
base_url = "https://api.x.ai/v1"
endpoints = ["api.x.ai:443"]
model = "grok-3"
keys = ["your_xai_key"]

# ── Mistral ──
[[upstream.profiles]]
id = "mistral"
provider = "mistral"
base_url = "https://api.mistral.ai/v1"
endpoints = ["api.mistral.ai:443"]
model = "mistral-large-latest"
keys = ["your_mistral_key"]

# ── Perplexity ──
[[upstream.profiles]]
id = "perplexity"
provider = "perplexity"
base_url = "https://api.perplexity.ai"
endpoints = ["api.perplexity.ai:443"]
model = "sonar-pro"
keys = ["your_perplexity_key"]

# ── Together AI ──
[[upstream.profiles]]
id = "together"
provider = "together"
base_url = "https://api.together.xyz/v1"
endpoints = ["api.together.xyz:443"]
model = "meta-llama/Llama-3.3-70B-Instruct-Turbo"
keys = ["your_together_key"]

# ── Fireworks AI ──
[[upstream.profiles]]
id = "fireworks"
provider = "fireworks"
base_url = "https://api.fireworks.ai/inference/v1"
endpoints = ["api.fireworks.ai:443"]
model = "accounts/fireworks/models/llama-v3p3-70b-instruct"
keys = ["your_fireworks_key"]

# ── Cerebras（世界最快推理）──
[[upstream.profiles]]
id = "cerebras"
provider = "cerebras"
base_url = "https://api.cerebras.ai/v1"
endpoints = ["api.cerebras.ai:443"]
model = "llama-3.3-70b"
keys = ["your_cerebras_key"]

# ── Cohere ──
[[upstream.profiles]]
id = "cohere"
provider = "cohere"
base_url = "https://api.cohere.com/v2"
endpoints = ["api.cohere.com:443"]
model = "command-a"
keys = ["your_cohere_key"]

# ── NVIDIA NIM ──
[[upstream.profiles]]
id = "nvidia"
provider = "nvidia"
base_url = "https://integrate.api.nvidia.com/v1"
endpoints = ["integrate.api.nvidia.com:443"]
model = "meta/llama-3.3-70b-instruct"
keys = ["your_nvidia_key"]

# ── Nebius AI ──
[[upstream.profiles]]
id = "nebius"
provider = "nebius"
base_url = "https://api.studio.nebius.ai/v1"
endpoints = ["api.studio.nebius.ai:443"]
model = "meta-llama/Meta-Llama-3.3-70B-Instruct"
keys = ["your_nebius_key"]

# ── SiliconFlow（硅基流动）──
[[upstream.profiles]]
id = "siliconflow"
provider = "siliconflow"
base_url = "https://api.siliconflow.cn/v1"
endpoints = ["api.siliconflow.cn:443"]
model = "Qwen/Qwen2.5-72B-Instruct"
keys = ["your_siliconflow_key"]

# ── Hyperbolic ──
[[upstream.profiles]]
id = "hyperbolic"
provider = "hyperbolic"
base_url = "https://api.hyperbolic.xyz/v1"
endpoints = ["api.hyperbolic.xyz:443"]
model = "meta-llama/Meta-Llama-3.1-70B-Instruct"
keys = ["your_hyperbolic_key"]

# ── OpenRouter（聚合网关，100+ 模型）──
[[upstream.profiles]]
id = "openrouter"
provider = "openrouter"
base_url = "https://openrouter.ai/api/v1"
endpoints = ["openrouter.ai:443"]
model = "anthropic/claude-sonnet-4"
keys = ["sk-or-your_openrouter_key"]

# ── Reka ──
[[upstream.profiles]]
id = "reka"
provider = "reka"
base_url = "https://api.reka.ai/v1"
endpoints = ["api.reka.ai:443"]
model = "reka-core"
keys = ["your_reka_key"]
```

### Step 2: 添加云平台供应商配置

```toml
# ═══════════════════════════════════════════════════════════
# 云平台供应商
# ═══════════════════════════════════════════════════════════

# ── Azure OpenAI ──
[[upstream.profiles]]
id = "azure-openai"
provider = "azure-openai"
base_url = "https://YOUR_RESOURCE.openai.azure.com"
endpoints = ["YOUR_RESOURCE.openai.azure.com:443"]
model = "gpt-4o"
keys = ["your_azure_key"]

# ── Amazon Bedrock ──
[[upstream.profiles]]
id = "bedrock"
provider = "bedrock"
base_url = "https://bedrock-runtime.us-east-1.amazonaws.com"
endpoints = ["bedrock-runtime.us-east-1.amazonaws.com:443"]
model = "anthropic.claude-sonnet-4-20250514-v1:0"
keys = ["your_aws_access_key:your_aws_secret_key"]

# ── Google Vertex AI ──
[[upstream.profiles]]
id = "vertex"
provider = "vertex"
base_url = "https://us-central1-aiplatform.googleapis.com/v1"
endpoints = ["us-central1-aiplatform.googleapis.com:443"]
model = "gemini-2.5-pro"
keys = ["your_vertex_access_token"]
```

### Step 3: 添加中国供应商配置

```toml
# ═══════════════════════════════════════════════════════════
# 中国供应商
# ═══════════════════════════════════════════════════════════

# ── Google Gemini ──
[[upstream.profiles]]
id = "gemini"
provider = "gemini"
base_url = "https://generativelanguage.googleapis.com/v1beta"
endpoints = ["generativelanguage.googleapis.com:443"]
model = "gemini-2.5-pro"
keys = ["your_gemini_key"]

# ── 阿里云通义千问 ──
[[upstream.profiles]]
id = "alibaba"
provider = "alibaba"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
endpoints = ["dashscope.aliyuncs.com:443"]
model = "qwen-max"
keys = ["sk-your_dashscope_key"]

# ── 百度千帆 ──
[[upstream.profiles]]
id = "qianfan"
provider = "qianfan"
base_url = "https://qianfan.baidubce.com/v2"
endpoints = ["qianfan.baidubce.com:443"]
model = "ernie-4.0-8k"
keys = ["your_qianfan_key"]

# ── 智谱 GLM ──
[[upstream.profiles]]
id = "glm"
provider = "glm"
base_url = "https://open.bigmodel.cn/api/paas/v4"
endpoints = ["open.bigmodel.cn:443"]
model = "glm-4-plus"
keys = ["your_glm_key"]

# ── Kimi (月之暗面) ──
[[upstream.profiles]]
id = "kimi"
provider = "kimi"
base_url = "https://api.moonshot.cn/v1"
endpoints = ["api.moonshot.cn:443"]
model = "kimi-k2.6"
keys = ["your_kimi_key"]

# ── Minimax ──
[[upstream.profiles]]
id = "minimax"
provider = "minimax"
base_url = "https://api.minimax.chat/v1"
endpoints = ["api.minimax.chat:443"]
model = "MiniMax-M2.5"
keys = ["your_minimax_key"]

# ── Moonshot AI ──
[[upstream.profiles]]
id = "moonshot"
provider = "moonshot"
base_url = "https://api.moonshot.cn/v1"
endpoints = ["api.moonshot.cn:443"]
model = "moonshot-v1-128k"
keys = ["your_moonshot_key"]

# ── 火山引擎 ──
[[upstream.profiles]]
id = "volcengine"
provider = "volcengine"
base_url = "https://ark.cn-beijing.volces.com/api/v3"
endpoints = ["ark.cn-beijing.volces.com:443"]
model = "doubao-seed-2-0-code-preview"
keys = ["your_volcengine_key"]

# ── 豆包 ──
[[upstream.profiles]]
id = "doubao"
provider = "doubao"
base_url = "https://ark.cn-beijing.volces.com/api/v3"
endpoints = ["ark.cn-beijing.volces.com:443"]
model = "doubao-1.5-pro-256k"
keys = ["your_doubao_key"]

# ── 腾讯混元 ──
[[upstream.profiles]]
id = "tencent"
provider = "tencent"
base_url = "https://api.hunyuan.cloud.tencent.com/v1"
endpoints = ["api.hunyuan.cloud.tencent.com:443"]
model = "hunyuan-turbos-latest"
keys = ["your_tencent_key"]

# ── 科大讯飞星火 ──
[[upstream.profiles]]
id = "iflytek"
provider = "iflytek"
base_url = "https://spark-api-open.xf-yun.com/v1"
endpoints = ["spark-api-open.xf-yun.com:443"]
model = "generalv3.5"
keys = ["your_iflytek_key"]

# ── 百川 ──
[[upstream.profiles]]
id = "baichuan"
provider = "baichuan"
base_url = "https://api.baichuan-ai.com/v1"
endpoints = ["api.baichuan-ai.com:443"]
model = "Baichuan4"
keys = ["your_baichuan_key"]

# ── 零一万物 Yi ──
[[upstream.profiles]]
id = "yi"
provider = "yi"
base_url = "https://api.lingyiwanwu.com/v1"
endpoints = ["api.lingyiwanwu.com:443"]
model = "yi-large"
keys = ["your_yi_key"]

# ── 阶跃星辰 StepFun ──
[[upstream.profiles]]
id = "stepfun"
provider = "stepfun"
base_url = "https://api.stepfun.com/v1"
endpoints = ["api.stepfun.com:443"]
model = "step-2-16k"
keys = ["your_stepfun_key"]

# ── 360 AI ──
[[upstream.profiles]]
id = "360ai"
provider = "360ai"
base_url = "https://api.360.cn/v1"
endpoints = ["api.360.cn:443"]
model = "360-gpt2-pro"
keys = ["your_360ai_key"]

# ── 商汤 SenseNova ──
[[upstream.profiles]]
id = "sensenova"
provider = "sensenova"
base_url = "https://api.sensenova.cn/v1/llm"
endpoints = ["api.sensenova.cn:443"]
model = "SenseChat-5"
keys = ["your_sensenova_key"]

# ── Coze (字节) ──
[[upstream.profiles]]
id = "coze"
provider = "coze"
base_url = "https://api.coze.com/v1"
endpoints = ["api.coze.com:443"]
model = "coze-bot"
keys = ["your_coze_key"]
```

### Step 4: 添加推理平台配置

```toml
# ═══════════════════════════════════════════════════════════
# 推理平台
# ═══════════════════════════════════════════════════════════

# ── DeepInfra ──
[[upstream.profiles]]
id = "deepinfra"
provider = "deepinfra"
base_url = "https://api.deepinfra.com/v1/openai"
endpoints = ["api.deepinfra.com:443"]
model = "meta-llama/Meta-Llama-3.1-70B-Instruct"
keys = ["your_deepinfra_key"]

# ── SambaNova ──
[[upstream.profiles]]
id = "sambanova"
provider = "sambanova"
base_url = "https://api.sambanova.ai/v1"
endpoints = ["api.sambanova.ai:443"]
model = "Meta-Llama-3.3-70B-Instruct"
keys = ["your_sambanova_key"]

# ── GitHub Models ──
[[upstream.profiles]]
id = "github-models"
provider = "github-models"
base_url = "https://models.inference.ai.azure.com"
endpoints = ["models.inference.ai.azure.com:443"]
model = "gpt-4o"
keys = ["your_github_pat"]

# ── HuggingFace ──
[[upstream.profiles]]
id = "huggingface"
provider = "huggingface"
base_url = "https://api-inference.huggingface.co/v1"
endpoints = ["api-inference.huggingface.co:443"]
model = "meta-llama/Meta-Llama-3.1-70B-Instruct"
keys = ["hf_your_huggingface_key"]

# ── Replicate ──
[[upstream.profiles]]
id = "replicate"
provider = "replicate"
base_url = "https://openai-proxy.replicate.com/v1"
endpoints = ["openai-proxy.replicate.com:443"]
model = "meta/llama-3.3-70b-instruct"
keys = ["r8_your_replicate_key"]

# ── Ollama Cloud ──
[[upstream.profiles]]
id = "ollama-cloud"
provider = "ollama-cloud"
base_url = "https://api.ollama.com/v1"
endpoints = ["api.ollama.com:443"]
model = "llama3.3:70b"
keys = ["your_ollama_cloud_key"]

# ── AI/ML API（200+ 模型聚合）──
[[upstream.profiles]]
id = "aimlapi"
provider = "aimlapi"
base_url = "https://api.aimlapi.com/v1"
endpoints = ["api.aimlapi.com:443"]
model = "gpt-4o"
keys = ["your_aimlapi_key"]

# ── Novita AI ──
[[upstream.profiles]]
id = "novita"
provider = "novita"
base_url = "https://api.novita.ai/v3/openai"
endpoints = ["api.novita.ai:443"]
model = "meta-llama/llama-3.3-70b-instruct"
keys = ["your_novita_key"]

# ── Chutes.ai ──
[[upstream.profiles]]
id = "chutes"
provider = "chutes"
base_url = "https://llm.chutes.ai/v1"
endpoints = ["llm.chutes.ai:443"]
model = "meta-llama/llama-3.3-70b-instruct"
keys = ["your_chutes_key"]

# ── Poe ──
[[upstream.profiles]]
id = "poe"
provider = "poe"
base_url = "https://api.poe.com/v1"
endpoints = ["api.poe.com:443"]
model = "Claude-Sonnet-4"
keys = ["your_poe_key"]

# ── Phind ──
[[upstream.profiles]]
id = "phind"
provider = "phind"
base_url = "https://https.api.phind.com/v1"
endpoints = ["https.api.phind.com:443"]
model = "Phind-70B"
keys = ["your_phind_key"]
```

## 验收标准

- [ ] 所有配置段格式正确，可被 `toml` 解析器解析
- [ ] 每个供应商的 `base_url` 和 `endpoints` 与 OmniRoute 参考一致
- [ ] `model` 字段使用各供应商的推荐默认模型
- [ ] `keys` 字段使用占位符（不包含真实密钥）

## 依赖

- 依赖任务 01（`UpstreamProvider` 枚举扩展，`provider` 字段值必须与枚举 `from_str` 匹配）

## 注意事项

- 配置文件中的 `provider` 值必须与 `UpstreamProvider::from_str()` 的输入匹配
- `base_url` 和 `endpoints` 以 OmniRoute 的 `providerRegistry.ts` 为准
- `keys` 使用占位符，用户需替换为自己的 API Key
- 某些供应商的 `endpoints` 可能需要从 `base_url` 自动推导
