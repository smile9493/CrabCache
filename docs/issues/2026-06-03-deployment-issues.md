# 2026-06-03 部署问题记录

## 问题 1: CodexMimo 管线 503 错误

### 现象
Codex 客户端请求 MiMo 模型时返回 `503 Service Unavailable`。

### 根因
- `CodexMimo` 管线被双重分类：既是 `is_codex_upstream_pipeline`，又是 `is_mimo_pipeline`
- Codex quota preflight 劫持了 key 获取路径
- MiMo pool 被误设 `codex_quota_cache`
- `looks_like_codex_oauth_key()` 拒绝所有 `auto-*` 格式的 MiMo key

### 修复（临时）
1. `request_filter.rs`: CodexMimo 跳过 codex preflight，走普通 `try_acquire_upstream_key`
2. `management_profiles.rs`: quota cache 仅分配给 `Codex` provider 的 pool

### 结构性修复（三条独立路径）
- `is_codex_upstream_pipeline()` 仅匹配 `CodexRelay` / `CodexDeepSeek`，不再包含 `CodexMimo`
- `CodexMimo` 仅通过 `is_mimo_pipeline()` 归类，使用 MiMo key pool 与 MiMo key binding
- `upstream_key_acquire_strategy()`：`CodexMimo` 固定走 `StandardUpstreamPool`（非 WHAM preflight）
- `uses_pool_scaled_retry_budget()`：`CodexMimo` / `MimoTokenPlanRelay` 与 Codex 桥接管线共享按 pool 缩放的 retry budget
- `response_filter.rs`: 成功时仅对 `is_codex_upstream_pipeline` 重置 Codex session binding，避免与 MiMo binding key 混用
- `rate_limit_retry.rs`: `codex_only = codex && !mimo` 限定 quota cache 与 OAuth key 轮换
- 单元测试：`upstream_key_acquire_strategy`、`uses_pool_scaled_retry_budget`、`codex_upstream_pipeline_excludes_codex_mimo`
- 路由不变：Codex 客户端 + `gpt-*` → `CodexRelay`；+ `deepseek-*` → `CodexDeepSeek`；+ `mimo-*` / `/v1/responses` 升级 → `CodexMimo`

### 状态: 已修复

---

## 问题 2: "Cannot start a runtime from within a runtime" 崩溃

### 现象
Gateway 和 Admin 容器启动后立即崩溃，exit code 139 (SIGSEGV)。

日志：
```
Cannot start a runtime from within a runtime.
This happens because a function (like `block_on`) attempted to block the current thread
while the thread is being used to drive asynchronous tasks.
```

### 根因
wuming 服务器上 Docker 镜像中的二进制是旧版本，存在 Tokio runtime 嵌套问题。`hot_update.py` 的 `docker cp` 替换二进制后，`docker compose up -d` 重建容器时覆盖了替换后的二进制。

### 解决方案
1. 本地构建轻量 Dockerfile（直接 COPY 预编译二进制，不从源码编译）
2. `docker save` + `scp` + `docker load` 传输镜像
3. 修改 compose 文件使用 `image:` 而非 `build:`
4. `docker compose --profile admin up -d` 全新部署

### 状态: 已解决

---

## 问题 3: `hot_update.py` 的 `docker cp` 卡死

### 现象
`hot_update.py` 脚本在执行 `docker cp` 时卡死，无响应。

### 根因
Docker 守护进程在长时间运行后出现状态异常，导致 `docker cp` 和 `docker exec` 命令挂起。

### 解决方案
1. `systemctl restart docker` 重启 Docker 守护进程
2. 改用镜像打包方式部署，避免依赖 `docker cp` 热替换

### 状态: 已解决

---

## 问题 4: Docker 守护进程不稳定

### 现象
多次 `docker cp`、`docker exec`、`docker inspect` 命令挂起或超时。

### 根因
wuming 服务器 Docker 守护进程运行 4 天后出现状态异常。

### 解决方案
重启 Docker 守护进程：`systemctl restart docker`

### 建议
- 定期重启 Docker 守护进程（如每周一次）
- 考虑使用 Docker healthcheck + auto-restart 策略

---

## 经验总结

1. **热更新 vs 镜像部署**: `hot_update.py` 适合小改动的快速部署，但存在 `docker cp` 卡死风险。大规模更新应使用镜像打包方式。
2. **Docker 守护进程稳定性**: 长时间运行后可能出现状态异常，需要定期维护。
3. **compose 文件管理**: 使用 `image:` 而非 `build:` 可以避免远程编译问题，同时确保部署的二进制版本可控。
4. **预编译镜像**: 本地编译 + `docker save/load` 是最可靠的部署方式，避免了远程编译和 `docker cp` 的不确定性。
