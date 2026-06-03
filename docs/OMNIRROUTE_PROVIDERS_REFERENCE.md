# OmniRoute 供应商参考文档

> 从 `_externals/OmniRoute` 项目扫描提取的完整供应商清单。
> 作为 CrabCache 供应商扩展的权威参考。
>
> **源文件**:
> - `_externals/OmniRoute/src/shared/constants/providers.ts`（供应商目录/UI 元数据）
> - `_externals/OmniRoute/open-sse/config/providerRegistry.ts`（API 注册表/端点配置）
>
> **扫描日期**: 2026-06-02

## 供应商统计

| 类别 | 数量 | 认证方式 | CrabCache 支持 |
|------|------|----------|---------------|
| API Key 供应商 | ~122 | Bearer Token | 部分（5→120+） |
| OAuth 供应商 | 16 | OAuth 2.0 (PKCE) | 仅 Codex |
| Web Cookie 供应商 | 18 | 浏览器 Cookie | 无（不计划支持） |
| No-Auth（免费）| 1 | 无需认证 | 无 |
| 本地/自托管 | 12 | 可选 API Key | 无（不计划支持） |
| 搜索 | 11 | API Key | 无（不计划支持） |
| 音频 | 7 | API Key | 无（不计划支持） |
| 图片/视频 | ~15 | API Key | 无（不计划支持） |
| 云代理 | 2 | 内部 | 无 |
| 云 Agent | 3 | API Key | 无 |
| **合计** | **~177** | | |

> CrabCache 仅计划支持 **API Key + OAuth** 类别的 LLM Chat 供应商。

### ID 映射说明

OmniRoute 供应商 ID 与 CrabCache `UpstreamProvider` 枚举名存在差异，主要映射规则：

| OmniRoute ID | CrabCache 枚举 | 说明 |
|--------------|----------------|------|
| `xiaomi-mimo` | `Mimo` | CrabCache 统一为 `mimo` |
| `pplx` | `Perplexity` | OmniRoute 用缩写，CrabCache 用全称 |
| `hyp` | `Hyperbolic` | 同上 |
| `ali` / `dashscope` | `Alibaba` | CrabCache 支持多别名 |
| `zhipu` / `bigmodel` | `Glm` | CrabCache 支持多别名 |
| `volc` | `Volcengine` | OmniRoute 用缩写 |

完整别名映射见 `crates/crab-pipeline/src/types.rs` 的 `UpstreamProvider::from_str()`。

---

## 一、API Key 供应商（LLM Chat，~120 个）

### 1.1 已有供应商（5 个）

| ID | 名称 | 格式 | base_url | 默认模型 | 特殊处理 |
|----|------|------|----------|----------|----------|
| `deepseek` | DeepSeek | openai | `https://api.deepseek.com` | `deepseek-v4-pro` | Reasoning/thinking 完整管线 |
| `xiaomi-mimo` | Xiaomi MiMo | openai | `https://api.xiaomimimo.com` | `mimo-v2.5-pro` | 会话级 key 绑定 |
| `openai` | OpenAI | openai | `https://api.openai.com/v1` | `gpt-4o` | 纯透传 |
| `codex` | OpenAI Codex | openai-responses | `https://chatgpt.com/backend-api/codex` | `codex-mini` | OAuth + Responses API |
| `anthropic` | Anthropic | claude | `https://api.anthropic.com` | `claude-sonnet-4` | x-api-key 认证 |

### 1.2 国际主流供应商（15 个）

| ID | 名称 | 别名 | 格式 | base_url | 默认模型 | 认证头 | 免费额度 |
|----|------|------|------|----------|----------|--------|----------|
| `groq` | Groq | `groq` | openai | `https://api.groq.com/openai/v1` | `llama-3.3-70b-versatile` | Authorization: Bearer | 30 RPM / 14.4K RPD |
| `xai` | xAI (Grok) | `xai` | openai | `https://api.x.ai/v1` | `grok-3` | Authorization: Bearer | 无 |
| `mistral` | Mistral | `mistral` | openai | `https://api.mistral.ai/v1` | `mistral-large-latest` | Authorization: Bearer | 免费 Experiment 层 |
| `perplexity` | Perplexity | `pplx` | openai | `https://api.perplexity.ai` | `sonar-pro` | Authorization: Bearer | 无 |
| `together` | Together AI | `together` | openai | `https://api.together.xyz/v1` | `meta-llama/Llama-3.3-70B-Instruct-Turbo` | Authorization: Bearer | $25 注册额度 |
| `fireworks` | Fireworks AI | `fireworks` | openai | `https://api.fireworks.ai/inference/v1` | `accounts/fireworks/models/llama-v3p3-70b-instruct` | Authorization: Bearer | $1 免费额度 |
| `cerebras` | Cerebras | `cerebras` | openai | `https://api.cerebras.ai/v1` | `llama-3.3-70b` | Authorization: Bearer | 1M tokens/天 |
| `cohere` | Cohere | `cohere` | openai | `https://api.cohere.com/v2` | `command-a` | Authorization: Bearer | 1000 次/月 |
| `nvidia` | NVIDIA NIM | `nvidia` | openai | `https://integrate.api.nvidia.com/v1` | `meta/llama-3.3-70b-instruct` | Authorization: Bearer | ~40 RPM |
| `nebius` | Nebius AI | `nebius` | openai | `https://api.studio.nebius.ai/v1` | `meta-llama/Meta-Llama-3.3-70B-Instruct` | Authorization: Bearer | ~$1 试用 |
| `siliconflow` | SiliconFlow | `siliconflow` | openai | `https://api.siliconflow.cn/v1` | `Qwen/Qwen2.5-72B-Instruct` | Authorization: Bearer | $1 + 永久免费模型 |
| `hyperbolic` | Hyperbolic | `hyp` | openai | `https://api.hyperbolic.xyz/v1` | `meta-llama/Meta-Llama-3.1-70B-Instruct` | Authorization: Bearer | $1-5 试用 |
| `openrouter` | OpenRouter | `openrouter` | openai | `https://openrouter.ai/api/v1` | `anthropic/claude-sonnet-4` | Authorization: Bearer | 免费模型 :free 后缀 |
| `reka` | Reka | `reka` | openai | `https://api.reka.ai/v1` | `reka-core` | Authorization: Bearer | $10/月免费 |
| `gemini` | Google Gemini | `gemini` | gemini | `https://generativelanguage.googleapis.com/v1beta` | `gemini-2.5-pro` | x-goog-api-key | 1500 请求/天 |

### 1.3 云平台供应商（7 个）

| ID | 名称 | 格式 | base_url | 认证方式 | 备注 |
|----|------|------|----------|----------|------|
| `azure-openai` | Azure OpenAI | openai | `https://{resource}.openai.azure.com` | api-key 头 | 需要 Azure 资源 |
| `azure-ai` | Azure AI Foundry | openai | `https://{resource}.services.ai.azure.com/openai/v1/` | api-key 头 | Foundry 端点 |
| `bedrock` | Amazon Bedrock | bedrock | `https://bedrock-runtime.{region}.amazonaws.com` | AWS 签名 | 原生 Converse API |
| `vertex` | Vertex AI | gemini | `https://{region}-aiplatform.googleapis.com/v1` | OAuth/SA | Google Cloud |
| `watsonx` | IBM watsonx | openai | `https://{region}.ml.cloud.ibm.com/ml/gateway/v1/` | Bearer | IBM Cloud |
| `oci` | OCI Generative AI | openai | `https://inference.generativeai.{region}.oci.oraclecloud.com/openai/v1/` | IAM Bearer | Oracle Cloud |
| `sap` | SAP AI Hub | openai | `{deploymentUrl}/chat/completions` | Bearer | SAP AI Core |

### 1.4 中国供应商（18 个）

| ID | 名称 | 别名 | base_url | 默认模型 | 免费额度 |
|----|------|------|----------|----------|----------|
| `alibaba` | 阿里通义千问 | `ali` | `https://dashscope-intl.aliyuncs.com` | `qwen-max` | 无 |
| `alibaba-cn` | 阿里通义（国内）| `ali-cn` | `https://dashscope.aliyuncs.com` | `qwen-max` | 无 |
| `qianfan` | 百度千帆 | `qianfan` | `https://qianfan.baidubce.com/v2` | `ernie-4.0-8k` | 免费 Speed/Lite |
| `glm` | 智谱 GLM Coding | `glm` | `https://open.bigmodel.cn/api/paas/v4` | `glm-4-plus` | 无 |
| `glm-cn` | 智谱 GLM（国内）| `glmcn` | `https://open.bigmodel.cn` | `glm-4-plus` | 无 |
| `glmt` | 智谱 GLM Thinking | `glmt` | `https://open.bigmodel.cn` | `glm-4-plus` | 无 |
| `kimi` | Kimi | `kimi` | `https://api.moonshot.cn/v1` | `kimi-k2.6` | 无 |
| `kimi-coding-apikey` | Kimi Coding (API Key) | `kmca` | `https://api.kimi.com/coding/v1/messages` | `kimi-k2.6` | x-api-key 认证 |
| `minimax` | Minimax Coding | `minimax` | `https://api.minimax.chat/v1` | `MiniMax-M2.5` | 无 |
| `minimax-cn` | Minimax（国内）| `minimax-cn` | `https://api.minimaxi.com` | `MiniMax-M2.5` | 无 |
| `moonshot` | Moonshot AI | `moonshot` | `https://api.moonshot.cn/v1` | `kimi-k2.6` | 无 |
| `volcengine` | 火山引擎 | `volcengine` | `https://ark.cn-beijing.volces.com/api/v3` | `doubao-seed-2-0-code-preview` | 无 |
| `doubao` | 豆包 | `doubao` | `https://doubao.com` | `doubao-1.5-pro` | 免费 Doubao 模型 |
| `tencent` | 腾讯混元 | `tencent` | `https://api.hunyuan.cloud.tencent.com/v1` | `hunyuan-turbos-latest` | 免费 Lite |
| `iflytek` | 科大讯飞星火 | `iflytek` | `https://spark-api-open.xf-yun.com/v1` | `generalv3.5` | 免费 Lite |
| `baichuan` | 百川 | `baichuan` | `https://api.baichuan-ai.com/v1` | `Baichuan4` | 免费模型 |
| `yi` | 零一万物 | `yi` | `https://api.lingyiwanwu.com/v1` | `yi-large` | 免费 Yi-Light |
| `stepfun` | 阶跃星辰 | `stepfun` | `https://api.stepfun.com/v1` | `step-2-16k` | 免费 Step-2 |
| `baidu` | 百度 ERNIE | `baidu` | `https://yiyan.baidu.com` | `ernie-speed` | 免费 Speed/Lite |
| `360ai` | 360 AI | `360ai` | `https://ai.360.cn` | `360-gpt2-pro` | 免费模型 |
| `sensenova` | 商汤 SenseNova | `sensenova` | `https://platform.sensenova.cn` | `SenseChat-5` | 免费模型 |
| `sparkdesk` | 星火 SparkDesk | `sparkdesk` | `https://xinghuo.xfyun.cn` | `spark-lite` | 免费 Lite |
| `coze` | Coze (字节) | `coze` | `https://api.coze.com/v1` | `coze-bot` | 免费平台 |
| `zai` | Z.AI (智谱) | `zai` | `https://open.bigmodel.cn` | `glm-4-plus` | 无 |

### 1.5 推理平台/聚合网关（60+ 个）

| ID | 名称 | 别名 | base_url | 免费额度 |
|----|------|------|----------|----------|
| `agentrouter` | AgentRouter | `agentrouter` | `https://api.agentrouter.org/v1` | $200 注册额度 |
| `command-code` | Command Code | `cmd` | `https://api.commandcode.ai/alpha` | 无 |
| `astraflow` | Astraflow (UCloud Global) | `astraflow` | `https://astraflow.ucloud-global.com` | 无 |
| `astraflow-cn` | Astraflow (UCloud China) | `astraflow-cn` | `https://astraflow.ucloud.cn` | 无 |
| `api-airforce` | Api.airforce | `af` | `https://api.airforce/v1` | 55 免费模型 |
| `qoder` | Qoder AI | `if` | 公共端点 | 免费层 |
| `bailian-coding-plan` | Alibaba Coding Plan | `bcp` | 阿里云 | 无 |
| `crof` | CrofAI | `crof` | `https://api.crof.ai` | 无 |
| `longcat` | LongCat AI | `lc` | `https://api.longcat.chat` | 50M tokens/天 |
| `pollinations` | Pollinations AI | `pol` | `https://text.pollinations.ai` | 免费，无需 key |
| `puter` | Puter AI | `pu` | `https://api.puter.com` | 免费（用户付费）|
| `uncloseai` | UncloseAI | `unc` | `https://api.uncloseai.com/v1` | 永久免费 |
| `replicate` | Replicate | `rep` | `https://openai-proxy.replicate.com/v1` | 免费社区模型 |
| `hackclub` | Hackclub AI | `hc` | `https://ai.hackclub.com` | 免费（会员）|
| `github-models` | GitHub Models | `ghm` | `https://models.inference.ai.azure.com` | 免费 GPT-5 等 |
| `cloudflare-ai` | Cloudflare Workers AI | `cf` | `https://api.cloudflare.com/client/v4/accounts/{id}/ai/v1` | 10K Neurons/天 |
| `scaleway` | Scaleway AI | `scw` | `https://api.scaleway.com/v1` | 1M 免费 tokens |
| `deepinfra` | DeepInfra | `deepinfra` | `https://api.deepinfra.com/v1/openai` | 免费注册额度 |
| `vercel-ai-gateway` | Vercel AI Gateway | `vag` | Vercel 端点 | 无 |
| `lambda-ai` | Lambda AI | `lambda` | `https://api.lambdalabs.com/v1` | 无 |
| `sambanova` | SambaNova | `samba` | `https://api.sambanova.ai/v1` | $5 免费额度 |
| `nscale` | nScale | `nscale` | `https://inference.api.nscale.com/v1` | $5 免费额度 |
| `ovhcloud` | OVHcloud AI | `ovh` | `https://ovhcloud.com/api/v1` | 无 |
| `baseten` | Baseten | `baseten` | `https://inference.baseten.co/v1` | $30 试用 |
| `publicai` | PublicAI | `publicai` | `https://api.publicai.co/v1` | 免费社区层 |
| `meta-llama` | Meta Llama API | `meta` | `https://api.llama.com/v1` | 无 |
| `v0-vercel` | v0 (Vercel) | `v0` | `https://api.v0.dev/v1` | 无 |
| `morph` | Morph | `morph` | `https://api.morphllm.com/v1` | 250K 免费/月 |
| `featherless-ai` | Featherless AI | `featherless` | `https://api.featherless.ai/v1` | 免费层 |
| `llm7` | LLM7.io | `llm7` | `https://api.llm7.io/v1` | 免费，无需注册 |
| `lepton` | Lepton AI | `lepton` | `https://api.lepton.ai/v1` | 免费层 |
| `kluster` | Kluster AI | `kluster` | `https://api.kluster.ai/v1` | $5 免费额度 |
| `friendliai` | FriendliAI | `friendli` | `https://inference.friendli.ai/v1` | 免费层 |
| `llamagate` | LlamaGate | `llamagate` | `https://api.llamagate.ai/v1` | 无 |
| `heroku` | Heroku AI | `heroku` | Heroku 端点 | 无 |
| `galadriel` | Galadriel | `galadriel` | `https://api.galadriel.com/v1` | 无 |
| `databricks` | Databricks | `databricks` | Databricks 端点 | 无 |
| `clarifai` | Clarifai | `clarifai` | `https://api.clarifai.com/v2/ext/openai/v1` | 无 |
| `snowflake` | Snowflake Cortex | `snowflake` | Snowflake 端点 | 无 |
| `wandb` | W&B Inference | `wandb` | W&B 端点 | 无 |
| `ai21` | AI21 Labs | `ai21` | `https://api.ai21.com/v1` | $10 试用 |
| `gigachat` | GigaChat (Sber) | `gigachat` | `https://gigachat.devices.sberbank.ru/api/v1` | 无 |
| `venice` | Venice.ai | `venice` | `https://api.venice.ai/v1` | 无 |
| `codestral` | Codestral | `codestral` | `https://codestral.mistral.ai/v1` | 无 |
| `upstage` | Upstage | `upstage` | `https://api.upstage.ai/v1` | 无 |
| `maritalk` | Maritalk | `maritalk` | `https://chat.maritaca.ai/api` | 无 |
| `modal` | Modal | `mdl` | Modal 端点 | $30/月免费 |
| `datarobot` | DataRobot | `datarobot` | DataRobot 端点 | 无 |
| `gitlawb` | Gitlawb (MiMo) | `glb` | `https://opengateway.gitlawb.com/v1` | 免费层 |
| `gitlawb-gmi` | Gitlawb (GMI Cloud) | `glb-gmi` | `https://opengateway.gitlawb.com/v1` | 免费层 |
| `inference-net` | Inference.net | `inet` | `https://inference.net/v1` | $25 免费额度 |
| `nanogpt` | NanoGPT | `nanogpt` | `https://nano-gpt.com/api/v1` | 无 |
| `predibase` | Predibase | `predibase` | `https://serving.app.predibase.com/v1` | $25 试用 |
| `bytez` | Bytez | `bytez` | `https://api.bytez.com/v1` | $1 免费/4 周 |
| `aimlapi` | AI/ML API | `aiml` | `https://api.aimlapi.com/v1` | $0.025/天 |
| `novita` | Novita AI | `novita` | `https://api.novita.ai/v3/openai` | $0.50 试用 |
| `piapi` | PiAPI | `pi` | `https://api.piapi.ai/v1` | 无 |
| `getgoapi` | GoAPI | `ggo` | `https://api.getgoapi.com/v1` | 无 |
| `laozhang` | LaoZhang AI | `lz` | `https://api.laozhang.ai/v1` | 无 |
| `glhf` | GLHF Chat | `glhf` | `https://glhf.chat/api/v1` | 免费开源模型 |
| `cablyai` | CablyAI | `cablyai` | `https://api.cablyai.com/v1` | 无 |
| `thebai` | TheB.AI | `thebai` | `https://api.theb.ai/v1` | 无 |
| `fenayai` | FenayAI | `fenayai` | `https://api.fenayai.com/v1` | 无 |
| `empower` | Empower | `empower` | `https://app.empower.dev/api/v1` | 无 |
| `nous-research` | Nous Research | `nous` | `https://inference-api.nousresearch.com/v1` | 50 RPM 免费 |
| `petals` | Petals | `petals` | `https://chat.petals.dev/api/v1` | 免费公共端点 |
| `poe` | Poe | `poe` | `https://api.poe.com/v1` | 无 |
| `gitlab` | GitLab Duo PAT | `gitlab` | GitLab 端点 | 无 |
| `chutes` | Chutes.ai | `chutes` | `https://llm.chutes.ai/v1` | 免费层 |
| `voyage-ai` | Voyage AI | `voyage` | `https://api.voyageai.com/v1` | 200M 免费 tokens |
| `jina-ai` | Jina AI | `jina` | `https://api.jina.ai/v1` | 10M 免费 tokens |
| `fal-ai` | Fal.ai | `fal` | `https://fal.run/v1` | 无 |
| `stability-ai` | Stability AI | `stability` | `https://api.stability.ai/v1` | 无 |
| `black-forest-labs` | Black Forest Labs | `bfl` | `https://api.bfl.ml/v1` | 无 |
| `recraft` | Recraft | `recraft` | `https://api.recraft.ai/v1` | 无 |
| `poolside` | Poolside | `poolside` | `https://api.poolside.ai/v1` | 免费 Laguna 模型 |
| `arcee-ai` | Arcee AI | `arcee` | `https://api.arcee.ai/v1` | 免费 Trinity 模型 |
| `inclusionai` | InclusionAI | `inclusion` | `https://api.inclusionai.com/v1` | 免费 Ling-2.6-flash |
| `liquid` | Liquid AI | `liquid` | `https://api.liquid.ai/v1` | 免费 LFM2.5 |
| `nomic` | Nomic | `nomic` | `https://api.nomic.ai/v1` | 免费 Embed API |
| `krutrim` | Krutrim | `krutrim` | `https://api.krutrim.ai/v1` | 免费层 |
| `monsterapi` | MonsterAPI | `monster` | `https://api.monsterapi.ai/v1` | 免费 GPU 推理 |
| `byteplus` | BytePlus ModelArk | `bpm` | BytePlus 端点 | 免费额度 |
| `bluesminds` | BluesMinds | `bm` | `https://api.bluesminds.com/v1` | 免费每日额度 |
| `freemodel-dev` | FreeModel.dev | `fmd` | `https://api.freemodel.dev/v1` | $300 免费额度 |
| `freeaiapikey` | FreeAIAPIKey | `faik` | `https://freeaiapikey.com/v1` | 折扣代理 |
| `blackbox` | Blackbox AI | `bb` | `https://api.blackbox.ai/v1` | 免费无限基础聊天 |
| `bazaarlink` | BazaarLink | `bzl` | `https://api.bazaarlink.ai/v1` | 免费 auto:free |
| `completions` | Completions.me | `cpl` | `https://api.completions.me/v1` | 免费无限 |
| `enally` | Enally AI | `enly` | `https://ai.enally.in/api/v1` | 免费学生/开发者 |
| `freetheai` | FreeTheAi | `fta` | `https://freetheai.xyz/v1` | 永久免费 |
| `opencode-zen` | OpenCode Zen | `opencode-zen` | `https://opencode.ai/zen/v1` | 无 |
| `opencode-go` | OpenCode Go | `opencode-go` | `https://opencode.ai/go/v1` | 无 |
| `phind` | Phind | `phind` | `https://https.api.phind.com/v1` | 免费代码搜索 |
| `huggingchat` | HuggingChat | `huggingchat` | `https://api-inference.huggingface.co/v1` | 免费开源模型 |
| `dify` | Dify | `dify` | Dify 实例端点 | 免费开源平台 |
| `sapio` | Sapio | `sapio` | Sapio 端点 | 无 |
| `huggingface` | HuggingFace | `hf` | `https://api-inference.huggingface.co/v1` | 免费推理 API |

---

## 二、OAuth 供应商（16 个，不计划支持）

| ID | 名称 | 备注 |
|----|------|------|
| `claude` | Claude Code | Anthropic OAuth |
| `codex` | OpenAI Codex | **已支持** |
| `cursor` | Cursor IDE | CrabCache 作为客户端检测 |
| `github` | GitHub Copilot | GitHub OAuth |
| `gitlab-duo` | GitLab Duo | GitLab OAuth |
| `windsurf` | Windsurf (Devin CLI) | Token 认证 |
| `cline` | Cline | OAuth |
| `qoder` | Qoder AI | OAuth |
| `qwen` | Qwen Code | 已废弃 |
| `gemini-cli` | Gemini CLI | Google OAuth |
| `agy` | Antigravity CLI | Google OAuth |
| `kiro` | Kiro AI | AWS Builder ID |
| `amazon-q` | Amazon Q | AWS Builder ID |
| `zed` | Zed IDE | OS Keychain |
| `trae` | Trae | ByteDance OAuth |
| `kilocode` | Kilo Code | OAuth |
| `kimi-coding` | Kimi Coding | OAuth + x-api-key |

---

## 三、Web Cookie 供应商（18 个，不计划支持）

| ID | 名称 |
|----|------|
| `chatgpt-web` | ChatGPT Web (Plus/Pro) |
| `grok-web` | Grok Web (Subscription) |
| `gemini-web` | Gemini Web (Free) |
| `perplexity-web` | Perplexity Web (Pro/Max) |
| `blackbox-web` | Blackbox Web (Subscription) |
| `muse-spark-web` | Muse Spark Web (Meta AI) |
| `claude-web` | Claude Web |
| `deepseek-web` | DeepSeek Web |
| `copilot-web` | Microsoft Copilot Web |
| `veoaifree-web` | Veo AI Free |
| `t3-web` | t3.chat (Pro/Free) |
| `inner-ai` | Inner.ai (Subscription) |
| `adapta-web` | Adapta.org |
| `duckduckgo-web` | DuckDuckGo AI Chat |
| `huggingchat` | HuggingChat (Free) |
| `phind` | Phind (Free) |
| `poe-web` | Poe Web (Subscription) |
| `venice-web` | Venice Web (Privacy) |
| `v0-vercel-web` | v0 Vercel Web |
| `kimi-web` | Kimi Web (Moonshot AI) |
| `doubao-web` | Doubao Web (ByteDance) |

---

## 四、其他类别（不计划支持）

### 4.1 No-Auth 供应商（1 个）

| ID | 名称 |
|----|------|
| `opencode` | OpenCode Free |

### 4.2 本地/自托管（12 个）

Ollama, LM Studio, vLLM, llama.cpp, Docker Model Runner, Jan, LocalAI, Text Generation WebUI, TabbyML, SGLang, Aphrodite, KoboldCpp

### 4.3 搜索供应商（11 个）

Brave Search, Exa, Tavily, Perplexity Search, SearXNG, SerpAPI, Serper, SearchAPI, You.com, Bing Search, Google Search

### 4.4 音频供应商（7 个）

Deepgram, ElevenLabs, Cartesia, PlayHT, OpenAI TTS, Google TTS, Azure Speech

### 4.5 图片/视频供应商（~15 个）

Stability AI, Fal.ai, Runway, Leonardo AI, Ideogram, Topaz, Black Forest Labs, Recraft, DALL-E, Midjourney, Veo AI, Haiper, Suno, Udio

---

## 五、OmniRoute 架构参考

### 5.1 供应商注册表模式

OmniRoute 使用 `RegistryEntry` 接口定义供应商配置：

```typescript
interface RegistryEntry {
  id: string;                    // 供应商 ID
  alias?: string;                // 短别名
  format: string;                // API 格式: "openai" | "claude" | "gemini" | "bedrock"
  executor: string;              // 执行器: "default" | "anthropic" | "gemini"
  baseUrl?: string;              // 默认 API 端点
  authType: string;              // 认证类型: "bearer" | "x-api-key" | "oauth"
  authHeader: string;            // 认证头: "Authorization" | "x-api-key"
  authPrefix?: string;           // 认证前缀: "Bearer " | "Token "
  headers?: Record<string, string>;  // 固定请求头
  models: RegistryModel[];       // 支持的模型列表
  chatPath?: string;             // 自定义聊天路径
  timeoutMs?: number;            // 超时时间
}
```

### 5.2 CrabCache 与 OmniRoute 的映射关系

| OmniRoute 概念 | CrabCache 对应 |
|----------------|---------------|
| `RegistryEntry.id` | `UpstreamProvider` 枚举变体 |
| `RegistryEntry.format` | `RequestPipeline`（GenericRelay = openai） |
| `RegistryEntry.baseUrl` | `UpstreamProfileConfig.base_url` |
| `RegistryEntry.authHeader` | 固定 `Authorization: Bearer` |
| `RegistryEntry.models` | 配置文件 `model` 字段 |
| `providerRegistry.ts` | `config/gateway.example.toml` |
| `providers.ts` | Dashboard UI 供应商列表 |

### 5.3 关键差异

1. **OmniRoute 是数据驱动**：添加供应商只需在注册表加一个条目
2. **CrabCache 是枚举驱动**：添加供应商需要修改枚举 + match 分支
3. **OmniRoute 支持多格式**：OpenAI/Claude/Gemini/Bedrock 通过 translator 层转换
4. **CrabCache 主要支持 OpenAI 格式**：其他格式走 GenericRelay 透传
