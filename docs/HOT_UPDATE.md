# 热更新 SOP（免重建镜像）

适用场景：`gateway` 与 `admin` 容器已经在运行，只想快速发布最新代码，不希望每次都执行 `docker compose build --no-cache`。

## 一键命令（推荐：本机构建 + 远程容器）

在**开发机**仓库根目录执行（详见 [`AGENTS.md`](../AGENTS.md)）：

```bash
cd /home/smile/github_project/CrabCache

# 内网部署机
python3 scripts/hot_update.py --target crabcache-deploy
# 或
make hot-update

# 公网 wuming
python3 scripts/hot_update.py --target wuming
```

旧版 bash（同机 Docker 或需手动 `export DOCKER_HOST=ssh://…`）：

```bash
./scripts/hot_update_runtime.sh   # make hot-update-legacy
```

## 脚本做了什么

`scripts/hot_update.py`（推荐）与 `scripts/hot_update_runtime.sh` 均按顺序完成：

1. 编译后端：`cargo build --release -p crab-gateway -p crab-admin`
2. 构建前端：`scripts/build_dashboard.sh` 生成 `crates/crab-dashboard/dist`
3. 校验主题标记：确认 `dist/index.html` 含 `theme-*`（避免前端资源不完整）
4. 复制产物到容器（`docker cp`）
5. 原子替换二进制：
   - `/app/crab-gateway`
   - `/app/crab-admin`
6. 清空并重建 Admin 容器内的 Dashboard dist 目录：
   - `/app/crates/crab-dashboard/dist`
7. 重启容器并健康检查（Python 脚本经 **SSH 在部署机** 访问 loopback）：
   - `http://127.0.0.1:9080/v1/ready`
   - Admin API 路由返回 JSON 而非 SPA `index.html`
8. 校验容器内外 SHA256 一致，确保更新确实生效

## 为什么不需要重建镜像

因为运行时只替换以下内容：

- `gateway`：`/app/crab-gateway` 二进制
- `admin`：`/app/crab-admin` 二进制 + `/app/crates/crab-dashboard/dist` 静态资源

镜像层（基础系统依赖、OpenSSL、运行时包）不变时，热更新即可覆盖绝大多数代码改动。

## 何时仍需要重建镜像

出现以下任一情况，建议执行完整 `docker compose build`：

- Dockerfile 变更（系统包、运行时库、基础镜像）
- 需要新增系统级依赖（例如新的动态库）
- 运行时启动参数、入口命令、镜像环境变量策略发生变化
- 想验证“从零构建”可复现（发布前演练）

## 常见问题

- **Trunk 报 `--no-color` 参数错误**
  - 现已在 `scripts/build_dashboard.sh` 处理：当 `NO_COLOR=1` 时自动规范为 `NO_COLOR=true`。

- **Admin 页面还是旧版本**
  - 以前只替换了 `crab-admin` 二进制，未同步 `dist`。
  - 现在脚本会强制重建并整体替换 `dist`，不会残留旧 hash 资源。

- **如何确认 gateway 更新成功**
  - 查看脚本输出的 `gateway_sha256`。
  - 对比容器内：
    ```bash
    docker exec crabcache-gateway-1 sha256sum /app/crab-gateway
    ```

## 新对话建议话术

在新会话里直接说：

> 请在仓库根目录执行 `python3 scripts/hot_update.py --target <主机>`，并报告健康检查与 SHA256 校验结果。

这样即使没有上下文，也能按标准流程执行。

## MiMo pre-header 发布验收（wuming / 内网）

分阶段热更新 **gateway**（配置 + 二进制），每阶段前后各跑一次：

```bash
ssh <host> 'docker exec crabcache-gateway-1 tail -10000 /app/logs/raw_capture/index.jsonl' \
  | python3 scripts/analyze_downstream_latency.py -
# 或用 crab-cli（推荐）：
crab-cli trace analyze --target wuming --tail 10000
crab-cli report --target wuming --compare-tail 200
```

| 阶段 | 配置 / 代码 | 成功标准（MiMo stream miss 样本） |
|------|-------------|-----------------------------------|
| 0 | `upstream_disable_keepalive=false`，`[features] connection_prewarm=true` | gap P50 较基线下降；TLS 复用可见 |
| 0 | Dashboard MiMo ≥2 Key、**不同 `account_id`** | 429 后 `gateway_upstream_key_*` 轮换；`acquire_excluding_account` 生效 |
| 1 | affinity sfp 重算（gateway 二进制） | 同 `session_fingerprint` backend 更集中 |
| 2 | `streaming_body_forward=true` 灰度 | gap P50 ≤3.5s（与 Phase 0 叠加目标 ≤3s）；gap/e2e ≤45%；`ttft` 非零占比不恶化 |
| 3 | `upstream_force_http1=false` + 响应 gzip 协商 | gap/e2e ≤45%；无 JSON/SSE 改写回归；探针见 `scripts/gate_upstream_encoding_probe.sh` |
| 4 | `[features] upstream_request_gzip=true`（可选） | 直接 MiMo gzip 探针 2xx；gap 较阶段 3 再降（大 body） |

**基线（2026-05-29，wuming，优化前）**：MiMo `mimo_relay` stream miss gap P50 **5212ms**（5870 条 raw_capture）。

Prometheus（可选）：`gateway_request_phase_latency_seconds{phase="upstream_response_headers"}` 与 `body_read_done` / `upstream_body_sent` 对照。

运维说明：[STREAMING_BODY_FORWARD.md](STREAMING_BODY_FORWARD.md)、[RUNTIME_LOG_FINDINGS.md](RUNTIME_LOG_FINDINGS.md)。
