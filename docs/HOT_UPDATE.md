# 热更新 SOP（免重建镜像）

适用场景：`gateway` 与 `admin` 容器已经在运行，只想快速发布最新代码，不希望每次都执行 `docker compose build --no-cache`。

## 一键命令

```bash
cd /opt/projct/CrabCache
make hot-update
```

等价命令：

```bash
./scripts/hot_update_runtime.sh
```

## 脚本做了什么

`scripts/hot_update_runtime.sh` 按顺序完成：

1. 编译后端：`cargo build --release -p crab-gateway -p crab-admin`
2. 构建前端：`scripts/build_dashboard.sh` 生成 `crates/crab-dashboard/dist`
3. 校验主题标记：确认 `dist/index.html` 含 `theme-*`（避免前端资源不完整）
4. 复制产物到容器（`docker cp`）
5. 原子替换二进制：
   - `/app/crab-gateway`
   - `/app/crab-admin`
6. 清空并重建 Admin 容器内的 Dashboard dist 目录：
   - `/app/crates/crab-dashboard/dist`
7. 重启容器并健康检查：
   - `http://127.0.0.1:9080/v1/ready`
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

> 请在仓库根目录执行 `make hot-update`，并报告健康检查与 SHA256 校验结果。

这样即使没有上下文，也能按标准流程执行。
