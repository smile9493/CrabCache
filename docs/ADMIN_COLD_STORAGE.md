# Admin 冷存储架构（PostgreSQL 权威）

> **硬性规则（所有 Key 池 / 凭证 / 账号相关改造必须遵守）**  
> 冷数据 → **PostgreSQL 长期权威**  
> 热数据 → **Gateway Redis / 进程内存**（运行时管道，可丢可重建）

## 分层定义

| 层 | 存储 | 内容 | 生命周期 |
|----|------|------|----------|
| **冷存（权威）** | Admin PostgreSQL | 上游 Profile Key 池、OAuth 完整凭证、客户端 `sk-cc-*` 元数据、域策略、模型目录等 | 长期；备份/恢复以 PG 为准 |
| **温存（镜像）** | `admin-state.json`、`/app/data/auths/*.json` | PG 的本地 JSON 镜像，便于无 PG 降级与工具兼容 | PG 成功写入后异步/镜像更新 |
| **热存（管道）** | Gateway Redis `crab:state:*` | 运行时 Key 池视图、TTL、路由、限流 cooldown、inflight | 从 Admin/PG **push** 重建；**不是**长期真相源 |

Redis 的职责：**多 Gateway 副本共享运行时控制面 + 快速 refresh**，不是账号/凭证/key 的归档库。

## PostgreSQL 表（Admin 冷存）

| 表 | 内容 |
|----|------|
| `upstream_profile_secrets` | 各 Profile 上游 Key 池：`key_id`、`secret`（含 Codex JWT）、`account_id`、`enabled` |
| `oauth_credentials` | Codex OAuth **完整** `TokenRecord` JSONB（含 `refresh_token`、`id_token`、metadata） |
| `upstream_pool_secrets` | 默认（DeepSeek）全局上游 Key 池 |
| `keys_meta` | 客户端 API Key 配额与用量元数据 |
| `domain_policies` | 域级策略 |
| `models` / `model_sync_state` | 模型目录 |
| `audit_log` | 管理审计 |
| `domain_usage` / `consumer_usage_monthly` | 用量冷存 |
| `trace_logs` | 脱敏请求 Trace（可选） |

## 写入顺序（强制）

任意 **Key 池 / 凭证** 变更必须按此顺序：

```
1. 更新 Admin 内存
2. COMMIT PostgreSQL（同步，失败须 warn；P0 路径应 retry）
3. flush admin-state.json（镜像）
4. PUT Gateway Management API → Redis 热管道
```

**禁止**：仅写 Redis / 仅写 Gateway 而不落 PG。  
**禁止**：Gateway → Admin 覆盖已有 PG 完整 Key 池（Codex JWT 池、PG 已 hydrate 的 Profile）。

## 启动顺序（Admin）

```
1. 连接 CRADMIN_PG_URL
2. hydrate upstream_profile_secrets ← PG
3. hydrate oauth_credentials ← PG → 镜像到 auth_dir
4. push 全量 Profile Key 池 → Gateway
5. reconcile Gateway 元数据（不覆盖 PG 权威池）
6. prepare_auth_dir（凭证 ↔ Key 池对齐）
```

## 代码入口

| 操作 | 模块 / 函数 |
|------|-------------|
| Profile Key 池 CRUD | `crates/crab-admin/src/upstream_profiles.rs` → `persist_profile_secrets_to_pg` |
| OAuth 凭证 save/list/get | `crates/crab-admin/src/credential_persist.rs` |
| PG CRUD | `crates/crab-admin/src/pg.rs` |
| 启动 hydrate | `AppState::try_connect_pg` + `hydrate_profile_secrets_from_pg` + `hydrate_credentials_from_pg` |

新增功能 checklist：

- [ ] 变更是否写入对应 PG 表？
- [ ] 是否先 PG 再 Gateway？
- [ ] 增量 Append 是否 merge 而非 blind replace？
- [ ] 启动是否从 PG hydrate？
- [ ] 文档 / 注释是否标明冷存表名？

## 环境变量

```bash
CRADMIN_PG_URL=postgres://user:pass@host:5432/crabcache_admin
CRADMIN_PG_MIGRATE_FROM_JSON=true   # 首次从 admin-state.json 灌入 PG
CRABCACHE_AUTH_DIR=/app/data/auths  # OAuth 本地镜像目录（非权威）
```

## 与 Gateway Redis 的关系

| 数据 | 冷存（PG） | 热管道（Redis） |
|------|------------|-----------------|
| Profile 上游 Key | ✅ `upstream_profile_secrets` | 运行时池 + cooldown |
| Codex OAuth 凭证 | ✅ `oauth_credentials` | 仅 access_token 在 Key 池 |
| 客户端 sk-cc-* | ✅ `keys_meta` + Redis 同步 | `crab:state:keys` |
| 响应缓存 L1 | ❌ | Redis `cache:*` |
| 限流 / inflight | ❌ | 进程内存 + Redis 快照 |

Gateway 重启：从 Redis 加载热状态；若 Redis 空，Admin 启动时 **从 PG push** 恢复 Key 池。

## 备份

生产备份 **必须包含 Admin PostgreSQL**（含 `upstream_profile_secrets` 与 `oauth_credentials`）。  
`admin_data` 卷与 `admin-state.json` 为镜像，不能替代 PG 备份。
