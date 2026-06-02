# Docker 部署检查清单

面向 **从零 `docker compose` 部署** 的配置对齐说明。Compose 文件：`docker-compose.yml`；推荐叠加：`docker-compose.admin.yml`。

## 服务与 Profile

| 命令 | 服务 |
|------|------|
| `docker compose up -d` | `gateway` + `redis` |
| `docker compose --profile admin up -d` | 上表 + `admin` + `postgres` |
| `+ -f docker-compose.admin.yml` | Gateway 等 Postgres；Admin 等 Gateway（推荐） |
| `--profile semantic` | 再加 `qdrant`（L2 需在配置中启用并挂载 ONNX 模型） |

## 环境变量（`.env`）

| 变量 | 必须？ | 说明 |
|------|--------|------|
| `CRABCACHE_GATEWAY_ADMIN_KEY` | 生产必须改 | Management API（`9080`，`x-gateway-admin-key`） |
| `CRABCACHE_ADMIN_KEY` | 生产必须改 | Dashboard 登录（与上 **不同**） |
| `CRADMIN_PG_PASSWORD` | admin profile 建议改 | 与 Postgres 容器、`CRADMIN_PG_URL` / `CRABCACHE_TRACE_PG_URL` 一致 |
| `CRADMIN_PG_URL` | 默认由 Compose 生成 | 未设时 Admin 启用 PG 冷存 |
| `CRABCACHE_TRACE_PG_URL` | admin 默认由 Compose 生成 | 仅 Gateway 时设 **空** 关闭 PG Trace |
| `CRABCACHE_BOOTSTRAP_CLIENT_KEYS` | 建议 | 启动注册 `sk-cc-*` |
| `CRABCACHE_UPSTREAM_KEYS` | 二选一 | 或在 Dashboard 配置 Key 池 |

密码只改 `CRADMIN_PG_PASSWORD` 时，**不要**在 `.env` 里写死带旧密码的 URL（Compose 会从密码拼 URL）。

## 推荐首次启动

```bash
cp .env.example .env
# 编辑 .env：两个 ADMIN_KEY、CRADMIN_PG_PASSWORD、BOOTSTRAP_CLIENT_KEYS 或上游 Key

docker compose -f docker-compose.yml -f docker-compose.admin.yml \
  --profile admin up -d --build

curl -sf http://127.0.0.1:9080/v1/ready
curl -sf -o /dev/null -w "%{http_code}\n" http://127.0.0.1:18001/
# Admin API 须为 JSON（非 HTML）：
curl -sS -o /dev/null -w "%{http_code} %{content_type}\n" \
  -H "x-admin-key: $CRABCACHE_ADMIN_KEY" \
  http://127.0.0.1:18001/api/admin/upstream/profiles/default/routing
```

## 冷启动顺序（Trace PG）

1. Postgres 就绪  
2. Admin 连接 PG 并 **迁移** `trace_logs` 等表（`CRADMIN_PG_URL` 默认已启用）  
3. Gateway Trace 写入 PG（表由 Admin 创建）

未使用 `docker-compose.admin.yml` 时，Gateway 可能与 Admin 迁移并行，Trace 插入会短暂失败并重试/仅写 JSONL，通常 Admin 起来后恢复。

## 仅 Gateway + Redis

```bash
cp .env.example .env
echo 'CRABCACHE_TRACE_PG_URL=' >> .env   # 或取消注释 .env.example 中的空值行
docker compose up -d
```

## Dashboard Admin Key

| 场景 | 行为 |
|------|------|
| Dashboard 改密 | 先 PG `system_config.admin_key`，再内存，再 `admin-key.txt` 镜像 |
| PG 连接但无 key | Admin 用文件/env bootstrap 写入 PG |
| PG 重启后 | 以 PG 值为准，覆盖 `admin-key.txt` |
| 无 PG | 仅文件 + env（与旧版一致） |

`CRABCACHE_ADMIN_KEY` env 仅首次 bootstrap 用；后续改密以 Dashboard 为准。

### 验收

```bash
# 1. 改密后查 PG（应有新值）
psql -h 127.0.0.1 -U crabcache -d crabcache_admin \
  -c "SELECT value FROM system_config WHERE key = 'admin_key'"

# 2. 文件镜像
docker exec crabcache-admin cat /app/data/admin-key.txt

# 3. 重启后仍可登录（PG 权威）
docker compose --profile admin restart admin

# 4. 删除文件后重启应从 PG 恢复
docker exec crabcache-admin rm /app/data/admin-key.txt
docker compose --profile admin restart admin
```

## 常见未对齐项

| 现象 | 原因 | 处理 |
|------|------|------|
| Dashboard Trace 无数据 | `CRADMIN_PG_URL` 未启用或迁移未完成 | 确认 admin 日志 `PostgreSQL connected` |
| `PG trace batch insert failed` | Gateway 早于迁移写 PG | 使用 `docker-compose.admin.yml` 或等待 Admin 启动 |
| Dashboard 502 | 未 `--profile admin` | `docker compose --profile admin up -d` |
| Admin API 返回 HTML | 二进制与 `dist` 不一致 | 本机 `make hot-update` 含 dashboard |
| Cursor 401 | 用了上游 Key 而非 `sk-cc-*` | Management 创建客户端 Key |
| 改 PG 密码后连不上 | URL 与 `CRADMIN_PG_PASSWORD` 不一致 | 只改密码变量或同时改 URL |
| L2 语义缓存无效 | `[semantic] enabled=false` 且无模型卷 | 下载 ONNX + `--profile semantic` |
| OAuth 失败 | OAuth 变量未进 admin 容器 | 写入 `.env`（admin 使用 `env_file`） |
| Keys 页反代地址错 | 无 1Panel 挂载 | 设 `CRABCACHE_GATEWAY_OPENRESTY_BASE_URL` |

## 反代与公网

Compose 默认 **127.0.0.1** 绑定端口。公网需 OpenResty/Nginx，见 [deploy-1panel-openresty.md](./deploy-1panel-openresty.md)。

## 相关文档

- [PERSISTENCE.md](./PERSISTENCE.md) — 冷存 / Redis / 多副本  
- [ADMIN_COLD_STORAGE.md](./ADMIN_COLD_STORAGE.md) — PG 表与写入顺序  
- [HOT_UPDATE.md](./HOT_UPDATE.md) — 热更新（不重建镜像）  
- [CURSOR_SETUP.md](./CURSOR_SETUP.md) — Cursor 与验收脚本  
