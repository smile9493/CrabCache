# CrabCache 网关地址配置指南

## 自动检测局域网地址

CrabCache 现在支持自动检测局域网地址，并在仪表盘中显示本地和局域网访问地址。

### 功能特性

1. **自动检测局域网 IP**
   - 自动识别主网络接口（eth、en、wlan、wl 等）
   - 过滤掉回环地址和链路本地地址
   - 支持多个网络接口显示

2. **双地址显示**
   - 本地地址：`http://127.0.0.1:8080`
   - 局域网地址：`http://192.168.x.x:8080`（自动检测）

3. **一键复制**
   - 点击复制按钮快速复制网关地址

### 使用方法

#### 1. 查看网关地址

访问仪表盘的 **Keys 页面**，您将看到：

```
本地地址: http://127.0.0.1:8080 [复制]
局域网地址: http://192.168.2.152:8080 [复制]
```

#### 2. 在局域网中使用

其他设备可以通过局域网地址访问网关：

```bash
# 在其他设备上使用
export OPENAI_API_BASE=http://192.168.2.152:8080/v1
export OPENAI_API_KEY=your-api-key
```

## SSL/HTTPS 配置（可选）

如果您需要使用 HTTPS，可以通过以下方式配置：

### 方案 1：使用反向代理（推荐）

使用 Nginx 或 Caddy 作为反向代理，处理 SSL 终止。

#### 使用 Caddy（最简单）

1. 安装 Caddy：
```bash
sudo apt install caddy
```

2. 创建 Caddyfile：
```
your-domain.com {
    reverse_proxy localhost:8080
}
```

3. 启动 Caddy：
```bash
caddy run --config Caddyfile
```

Caddy 会自动申请和续期 Let's Encrypt SSL 证书。

#### 使用 Nginx

1. 安装 Nginx：
```bash
sudo apt install nginx
```

2. 配置 Nginx：
```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 方案 2：本地自签名证书

用于开发和测试：

```bash
# 生成自签名证书
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes

# 使用 Nginx 或其他反向代理配置 HTTPS
```

### 方案 3：使用 Cloudflare Tunnel

如果您有域名，可以使用 Cloudflare Tunnel：

1. 安装 cloudflared：
```bash
wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
sudo dpkg -i cloudflared-linux-amd64.deb
```

2. 配置隧道：
```bash
cloudflared tunnel login
cloudflared tunnel create crabcache
cloudflared tunnel route dns crabcache your-domain.com
```

3. 运行隧道：
```bash
cloudflared tunnel run --url http://localhost:8080 crabcache
```

## 网络接口信息 API

### API 端点

```
GET /api/admin/network/info
```

### 响应示例

```json
{
  "primary_ip": "192.168.2.152",
  "all_ips": [
    {
      "name": "enx00e04c402428",
      "ip": "192.168.2.152",
      "is_primary": true
    },
    {
      "name": "docker0",
      "ip": "172.17.0.1",
      "is_primary": false
    }
  ],
  "gateway_url": "http://127.0.0.1:8080",
  "gateway_url_lan": "http://192.168.2.152:8080"
}
```

### 字段说明

- `primary_ip`: 主要局域网 IP 地址
- `all_ips`: 所有网络接口列表
- `gateway_url`: 本地回环地址
- `gateway_url_lan`: 局域网访问地址（仅私网 IP；自动排除 `docker0`/`br-`/`veth` 等虚拟接口）
- `gateway_url_openresty`: OpenResty 反代后的客户端 Base URL（自动解析 1Panel `conf.d` 中 `proxy_pass http://127.0.0.1:8080` 的 `server` 块，或环境变量覆盖）

### 环境变量（Admin 进程）

| 变量 | 默认 | 说明 |
|------|------|------|
| `CRABCACHE_GATEWAY_CLIENT_PORT` | `8080` | 客户端请求网关端口 |
| `CRABCACHE_GATEWAY_CLIENT_LAN_HOST` | — | 覆盖局域网主机（Docker 建议设为宿主机 LAN IP） |
| `CRABCACHE_GATEWAY_OPENRESTY_BASE_URL` | — | 覆盖 `gateway_url_openresty`（如 `https://v4.example.com:18000`） |
| `CRABCACHE_GATEWAY_CLIENT_BASE_URL` | — | 同上（遗留别名） |
| `CRABCACHE_OPENRESTY_CONF_DIR` | — | 扫描的 Nginx 配置目录（Docker 默认 `/host/openresty/conf.d`） |
| `CRABCACHE_HTTPS` | `false` | 本地/局域网 URL 的 `http`/`https` 前缀 |

## 故障排查

### 1. 检测不到局域网地址

**原因**：无合格私网接口（例如仅有 `docker0` 的 `172.17.0.1`），或 Admin 运行在容器内

**解决方法**：
- 检查网络连接：`ip addr show`
- 确保网络接口已启动：`sudo ip link set eth0 up`
- Docker：在 Admin 环境变量中设置 `CRABCACHE_GATEWAY_CLIENT_LAN_HOST=<宿主机局域网 IP>`

### 2. 局域网无法访问

**原因**：防火墙阻止了端口访问

**解决方法**：
```bash
# 开放 8080 端口（网关）
sudo ufw allow 8080/tcp

# 开放 3000 端口（仪表盘）
sudo ufw allow 3000/tcp

# 重新加载防火墙
sudo ufw reload
```

### 3. Docker 网络问题

Admin 容器内自动探测到的往往是容器网桥 IP，**不能**作为局域网网关地址。推荐：

```bash
# docker-compose admin 服务
CRABCACHE_GATEWAY_CLIENT_LAN_HOST=192.168.1.100  # 宿主机在局域网中的 IP
```

若需从局域网直连网关进程，还需将网关端口映射到 `0.0.0.0`（或使用 `network_mode: host`），仅映射 `127.0.0.1:8080` 时局域网设备无法访问。

## 最佳实践

1. **生产环境**：使用反向代理 + Let's Encrypt SSL 证书
2. **开发环境**：使用 HTTP + 局域网地址即可
3. **安全考虑**：
   - 不要在公网暴露 8080 端口
   - 使用防火墙限制访问
   - 启用 API Key 认证

## 配置示例

### 完整的生产环境配置

```bash
# 1. 启动 CrabCache 网关
./crab-gateway config/gateway.toml

# 2. 启动 CrabCache 管理面板
./crab-admin

# 3. 使用 Caddy 配置 HTTPS
cat > Caddyfile << EOF
gateway.yourdomain.com {
    reverse_proxy localhost:8080
}

admin.yourdomain.com {
    reverse_proxy localhost:3000
}
EOF

# 4. 启动 Caddy
caddy run --config Caddyfile
```

现在您可以通过以下地址访问：
- 网关：`https://gateway.yourdomain.com`
- 管理面板：`https://admin.yourdomain.com`
