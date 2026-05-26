# CrabCache 文档

欢迎查阅 CrabCache 项目文档。**CrabCache 以 [Cloudflare Pingora](https://github.com/cloudflare/pingora) 为代理核心**，在 `ProxyHttp` 过滤器链上实现多供应商 LLM API 网关能力（SSE 流式、粘滞路由、Reasoning、控制面）。以 DeepSeek V4 为重点参考实现，专注缓存优化与可观测透明分析。详见 [上游前缀缓存（L3）](DEEPSEEK_PREFIX_CACHE.md)。

## 快速导航

### 🚀 入门指南

| 文档 | 说明 |
|------|------|
| [Cursor + DeepSeek 接入指南](CURSOR_SETUP.md) | 如何使用 Cursor IDE 通过 CrabCache 访问 DeepSeek |
| [从 new-api 迁移](NEW_API_MIGRATION.md) | 从 new-api + deepseek-cursor-proxy 双栈迁移到单栈 CrabCache |
| [Agent 客户端 Key 迁移](AGENT_CLIENT_KEY_MIGRATION.md) | 客户端 API Key 迁移与升级说明 |

### 🧠 供应商集成

| 文档 | 说明 |
|------|------|
| [DeepSeek 上游前缀缓存（L3）](DEEPSEEK_PREFIX_CACHE.md) | 利用 DeepSeek 服务端 KV 前缀缓存的实践指南 |
| [Cursor Proxy 协议对照](DEEPSEEK_CURSOR_PROXY_PARITY.md) | CrabCache 与 deepseek-cursor-proxy 的功能对照 |
| [Cursor DeepSeek 吸收计划](CURSOR_DEEPSEEK_ABSORPTION_PLAN.md) | Go 侧模型别名等功能的迁移路线图 |

### ⚙️ 运维管理

| 文档 | 说明 |
|------|------|
| [多租户隔离](MULTI_TENANT.md) | project_id / DeepSeek user_id 租户隔离机制 |
| [持久化指南](PERSISTENCE.md) | 各功能数据的存储位置、多实例部署与备份 |
| [推理内容存储](REASONING_STORE.md) | ReasoningStore 与稳定会话 scope 机制 |
| [可观测性](OBSERVABILITY.md) | Prometheus 指标、Admin Dashboard、影子日志 |

### 🚢 部署指南

| 文档 | 说明 |
|------|------|
| [1Panel + OpenResty 部署](deploy-1panel-openresty.md) | 使用 1Panel 和 OpenResty 反代部署 |
| [网络地址配置](network-config.md) | 网关地址自动检测与局域网配置 |
| [自签名证书](self-signed-cert.md) | 为管理面板生成自签名 SSL 证书 |

---

## 项目资源

- [GitHub 仓库](https://github.com/smile9493/CrabCache)
- [项目 README](../README.md) — 架构概述、快速开始、配置参考