# 1Panel + OpenResty 发布 CrabCache（域名 + 端口，无公网 80/443）

适用于：**公网 IP 未开放 80/443**，仅能通过 **`域名:端口`** 访问；OpenResty 使用 **host 网络**（容器 `1Panel-openresty-*`），CrabCache 网关监听 **`127.0.0.1:8080`**。

测试域名：**`your-domain.example.com`**

## 端口规划（推荐）

| 端口 | 用途 | 反代上游 |
|------|------|----------|
| **18000** | OpenAI 兼容 API（客户端 Base URL） | `http://127.0.0.1:8080` |
| **18010** | Admin 仪表盘（Leptos + crab-admin） | `http://127.0.0.1:18001` |

防火墙 / 安全组需放行 **18000、18010**（TCP）。**不要**将 9080（Management）、Redis 暴露到公网。

## 客户端与前端地址

```text
# Cursor / OpenAI SDK / 其他 Agent
Base URL:  https://your-domain.example.com:18000/v1
API Key:   网关客户端密钥（如 CRABCACHE_BOOTSTRAP_CLIENT_KEYS 或 sk-cc-*）

# 浏览器打开仪表盘
https://your-domain.example.com:18010
```

仪表盘前端请求同源的 `/api/admin/*`，经 **18010** 反代到本机 `18001` 即可，**无需**改 WASM 里的 API 路径。

## 证书（1Panel 已有证书时）

站点证书目录（宿主机与 OpenResty 容器内路径一致）：

```text
/opt/1panel/www/sites/your-domain.example.com/ssl/fullchain.pem
/opt/1panel/www/sites/your-domain.example.com/ssl/privkey.pem
```

在 1Panel 为站点申请/绑定证书后，将面板导出的 **完整链** 与 **私钥** 放到上述路径（或确认面板已写入该目录），然后重载 OpenResty：

```bash
docker exec 1Panel-openresty-yLy6 openresty -t
docker exec 1Panel-openresty-yLy6 openresty -s reload
```

当前服务器若 `ssl/` 为空，可先用自签证书做联调；客户端需信任证书或临时关闭校验（仅测试）。

参考配置（已用于本机联调）：**`/opt/1panel/www/conf.d/your-domain.example.com.conf`**  
仓库镜像示例：**[`deploy/nginx/crabcache-openresty-1panel.example.conf`](../deploy/nginx/crabcache-openresty-1panel.example.conf)**

## 前置条件

```bash
cd /path/to/CrabCache
docker compose --profile admin up -d
curl -sf http://127.0.0.1:8080/ready
curl -sf -o /dev/null http://127.0.0.1:18001/   # admin 本地
```

`.env` 建议包含：

```env
CRABCACHE_BOOTSTRAP_CLIENT_KEYS=sk-cc-your-client-key-here
CRABCACHE_GATEWAY_ADMIN_KEY=<管理密钥>
```

## 1Panel 常见坑

### 1) 不要用 80/443 做联调

本环境公网 **无 80/443 出口**；用 **`https://域名:18000`**，勿写无端口的 `https://域名/v1`。

### 2) `proxy_pass` 必须指向 8080 / 18001

错误示例：`http://127.0.0.1:18080` → 站点 `error.log` 会出现 `connect() failed (111: Connection refused)`，浏览器显示 **502 Bad Gateway**。

仪表盘需 **`docker compose --profile admin`** 启动；仅起 `gateway` 时 `18001` 无进程监听，刷新 `https://域名:18010` 会偶发 502。详见 [OBSERVABILITY.md](./OBSERVABILITY.md)「Dashboard 502 / 503 troubleshooting」。

### 3) 内网用公网域名访问可能 403

部分 WAF 对「公网域名 + RFC1918 来源」返回 `Rejected request from RFC1918 IP...`。请用 **外网客户端** 或本机：

```bash
curl -sk https://127.0.0.1:18000/ready -H 'Host: your-domain.example.com'
```

### 4) 流式 SSE

`location` 中保留 **`proxy_buffering off;`**、`proxy_read_timeout 300s`。

## 稳定 `x-conversation-id`（ReasoningStore）

Cursor / 子代理往往不带会话头时，网关会用 **`client:<sk-cc>`** 作 ReasoningStore scope；仍建议在 **18000 API** 的 `location` 中注入稳定 `x-conversation-id`，使同一线程多轮 tool 历史更易命中 Redis。

仓库示例（注释形式，按环境启用其一）：

- **Cookie**：`proxy_set_header x-conversation-id $cookie_<your_cookie>;`
- **Authorization 派生（示例配置默认启用）**：[`deploy/nginx/crabcache-openresty-1panel.example.conf`](../deploy/nginx/crabcache-openresty-1panel.example.conf) 顶部 `map` + `proxy_set_header x-conversation-id $crabcache_conv_id_final;`（可被 cookie `crabcache_thread_id` 覆盖）

详见 [`deploy/nginx/crabcache-openresty-1panel.example.conf`](../deploy/nginx/crabcache-openresty-1panel.example.conf) 与 **[REASONING_STORE.md](REASONING_STORE.md)**。

网关内置兜底：无会话头时使用 `req:<SHA256 前 16 位>`，不依赖 OpenResty。

## 验证清单

一键脚本（公网域名 + 端口）：

```bash
export DOMAIN=your-domain.example.com
export CLIENT_API_KEY='你的客户端密钥'
bash scripts/verify_domain_port.sh
```

| 检查项 | 命令 / 预期 |
|--------|-------------|
| API HTTPS | `curl -sk https://127.0.0.1:18000/ready -H 'Host: your-domain.example.com'` → **200** |
| 仪表盘 | `curl -sk -o /dev/null -w '%{http_code}\n' https://127.0.0.1:18010/` → **200** |
| 鉴权 | `curl -sk https://127.0.0.1:18000/v1/models -H 'Authorization: Bearer <客户端密钥>'` → **200** |
| 外网 | 在放行 18000/18010 后，用手机流量访问上述 URL |

---

**安全提示**：Management API（9080）仅限内网或 SSH 隧道。
