# CrabCache 本地自签名证书使用指南

## 快速开始

### 1. 生成自签名证书

```bash
./scripts/generate-cert.sh
```

这将在 `certs/` 目录下生成：
- `cert.pem` - SSL 证书
- `key.pem` - 私钥

### 2. 启动 HTTPS 服务器

```bash
./scripts/start-https.sh
```

### 3. 访问仪表盘

在浏览器中打开：
```
https://localhost:3000
```

## 详细说明

### 证书生成

#### 默认配置（localhost）

```bash
./scripts/generate-cert.sh
```

#### 自定义域名

```bash
./scripts/generate-cert.sh your-domain.local
```

#### 自定义有效期（天数）

```bash
./scripts/generate-cert.sh localhost 730
```

### 启动选项

#### HTTPS 模式（推荐）

```bash
# 方式 1: 使用启动脚本
./scripts/start-https.sh

# 方式 2: 直接运行
cargo run --release --bin crab-admin -- --https

# 方式 3: 指定证书路径
cargo run --release --bin crab-admin -- \
  --cert /path/to/cert.pem \
  --key /path/to/key.pem
```

#### HTTP 模式

```bash
# 方式 1: 使用启动脚本
./scripts/start-http.sh

# 方式 2: 直接运行
cargo run --release --bin crab-admin

# 方式 3: 自定义端口
cargo run --release --bin crab-admin -- --listen 0.0.0.0:8080
```

### 命令行参数

| 参数 | 简写 | 说明 | 示例 |
|------|------|------|------|
| `--listen` | `-l` | 监听地址 | `--listen 0.0.0.0:8080` |
| `--cert` | `-c` | 证书文件路径 | `--cert certs/cert.pem` |
| `--key` | `-k` | 私钥文件路径 | `--key certs/key.pem` |
| `--https` | - | 使用默认证书路径启动 HTTPS | `--https` |

## 浏览器信任证书

### Chrome / Edge

1. 访问 `https://localhost:3000`
2. 看到安全警告时，点击 **"Advanced"**
3. 点击 **"Proceed to localhost (unsafe)"**

### Firefox

1. 访问 `https://localhost:3000`
2. 点击 **"Advanced"**
3. 点击 **"Accept the Risk and Continue"**

### Safari

1. 访问 `https://localhost:3000`
2. 点击 **"Show Details"**
3. 点击 **"visit this website"**

## 系统级信任证书（可选）

### Linux (Ubuntu/Debian)

```bash
# 复制证书到系统证书目录
sudo cp certs/cert.pem /usr/local/share/ca-certificates/crabcache.crt

# 更新证书库
sudo update-ca-certificates

# 重启浏览器
```

### macOS

```bash
# 添加证书到系统钥匙串
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain certs/cert.pem
```

### Windows

```powershell
# 以管理员身份运行 PowerShell
certutil -addstore -f "ROOT" certs\cert.pem
```

## API 访问

### 使用 HTTPS

```bash
# 忽略证书验证（开发环境）
curl -k https://localhost:3000/api/admin/metrics

# 或使用 --insecure
curl --insecure https://localhost:3000/api/admin/metrics
```

### Python 示例

```python
import requests

# 忽略 SSL 警告
import urllib3
urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

response = requests.get('https://localhost:3000/api/admin/metrics', verify=False)
print(response.json())
```

### Node.js 示例

```javascript
const https = require('https');
const fs = require('fs');

const options = {
  hostname: 'localhost',
  port: 3000,
  path: '/api/admin/metrics',
  method: 'GET',
  rejectUnauthorized: false  // 忽略证书验证
};

const req = https.request(options, (res) => {
  let data = '';
  res.on('data', (chunk) => { data += chunk; });
  res.on('end', () => { console.log(JSON.parse(data)); });
});

req.end();
```

## 网络信息 API

访问网络信息 API 会自动检测 HTTPS 模式：

```bash
# HTTP 模式
curl http://localhost:3000/api/admin/network/info
# 返回: {"gateway_url": "http://127.0.0.1:8080", ...}

# HTTPS 模式（设置环境变量）
export CRABCACHE_HTTPS=1
curl -k https://localhost:3000/api/admin/network/info
# 返回: {"gateway_url": "https://127.0.0.1:8080", ...}
```

## 证书管理

### 查看证书信息

```bash
# 查看证书详情
openssl x509 -in certs/cert.pem -text -noout

# 查看证书有效期
openssl x509 -in certs/cert.pem -noout -dates

# 验证证书和私钥是否匹配
openssl x509 -noout -modulus -in certs/cert.pem | openssl md5
openssl rsa -noout -modulus -in certs/key.pem | openssl md5
```

### 续期证书

```bash
# 重新生成证书（覆盖旧证书）
./scripts/generate-cert.sh localhost 365
```

### 备份证书

```bash
# 备份证书文件
tar -czf certs-backup-$(date +%Y%m%d).tar.gz certs/
```

## 安全建议

### ⚠️ 仅用于开发和测试

自签名证书不适用于生产环境，因为：
- 浏览器会显示安全警告
- 无法防止中间人攻击
- 不被公共 CA 信任

### ✅ 生产环境推荐

对于生产环境，请使用：
1. **Let's Encrypt** - 免费的自动化证书
2. **商业 SSL 证书** - 付费的可信证书
3. **内部 CA** - 企业内部的证书颁发机构

## 故障排查

### 证书文件找不到

```
Error: No such file or directory (os error 2)
```

**解决方法**：
```bash
./scripts/generate-cert.sh
```

### 端口被占用

```
Error: Address already in use (os error 98)
```

**解决方法**：
```bash
# 查找占用端口的进程
lsof -i :3000

# 终止进程
kill -9 <PID>

# 或使用其他端口
cargo run --release --bin crab-admin -- --listen 0.0.0.0:3001 --https
```

### 浏览器无法访问

**可能原因**：
1. 防火墙阻止了端口
2. 服务未启动
3. 证书路径错误

**检查步骤**：
```bash
# 1. 检查服务是否运行
ps aux | grep crab-admin

# 2. 检查端口是否监听
netstat -tulpn | grep 3000

# 3. 测试本地连接
curl -k https://localhost:3000

# 4. 检查防火墙
sudo ufw status
```

## 完整示例

### 开发环境

```bash
# 1. 生成证书
./scripts/generate-cert.sh localhost

# 2. 启动 HTTPS 服务
./scripts/start-https.sh

# 3. 在另一个终端测试
curl -k https://localhost:3000/api/admin/metrics | jq
```

### 局域网访问

```bash
# 1. 生成证书（使用本机 IP）
./scripts/generate-cert.sh 192.168.1.100

# 2. 启动服务
./scripts/start-https.sh

# 3. 在其他设备上访问
# https://192.168.1.100:3000
```

## 参考资料

- [OpenSSL 文档](https://www.openssl.org/docs/)
- [TLS/SSL 最佳实践](https://ssl-config.mozilla.org/)
- [自签名证书安全指南](https://www.ssl.com/how-to/create-a-self-signed-certificate/)
