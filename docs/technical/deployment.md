# 部署与运维

## 构建产物

### 后端二进制

```bash
cargo build --release --workspace
```

产物位置：

- `target/release/web-platform` — 管理平台主进程
- `target/release/symphony-platform` — 编排引擎（由 web-platform 作为子进程拉起）

两个二进制均为静态链接，无外部运行时依赖，可直接复制到目标机器运行。

### 前端静态文件

```bash
cd web-frontend
npm install
npm run build
```

产物位置：`web-frontend/dist/`，包含所有静态资源（HTML、JS、CSS）。

---

## 环境变量配置

| 变量 | 必填 | 说明 | 示例 |
|------|------|------|------|
| `JWT_SECRET` | 是 | JWT 签名密钥，至少 32 字符 | `your-production-secret-key-here` |
| `ENCRYPTION_KEY` | 是 | AES-GCM 256-bit 密钥，Base64 编码的 32 字节 | `MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=` |
| `DATABASE_URL` | 否 | SQLite 数据库文件路径 | `/data/symphony.db` |
| `SERVER_HOST` | 否 | 监听地址，默认 `0.0.0.0` | `127.0.0.1` |
| `SERVER_PORT` | 否 | 监听端口，默认 `3000` | `3000` |
| `SYMPHONY_BIN` | 否 | symphony-platform 二进制路径，默认 `symphony-platform` | `/usr/local/bin/symphony-platform` |
| `SYMPHONY_WORKSPACE_ROOT` | 否 | 工作空间根目录，默认 `./workspaces` | `/data/workspaces` |
| `ADMIN_INIT_PASSWORD` | 否 | 首次启动时创建 admin 用户的初始密码 | `change-me-on-first-login` |
| `RUST_LOG` | 否 | 日志级别，默认 `web_platform=info` | `web_platform=info,tower_http=warn` |
| `http_proxy` / `HTTP_PROXY` | 否 | HTTP 代理 | `http://proxy.example.com:8080` |
| `https_proxy` / `HTTPS_PROXY` | 否 | HTTPS 代理 | `http://proxy.example.com:8080` |
| `all_proxy` / `ALL_PROXY` | 否 | 全局代理 | `socks5://proxy.example.com:1080` |

生成安全的 `ENCRYPTION_KEY`：

```bash
openssl rand -base64 32
```

生成安全的 `JWT_SECRET`：

```bash
openssl rand -hex 32
```

---

## 数据库初始化

首次启动时，web-platform 会自动通过 Refinery 执行所有数据库迁移，无需手动初始化。确保 `DATABASE_URL` 指向的目录存在且有写权限：

```bash
mkdir -p /data
# 启动时自动创建 symphony.db 并执行迁移
```

---

## 启动方式

### 直接运行

```bash
export JWT_SECRET="your-production-secret"
export ENCRYPTION_KEY="your-base64-encoded-32-byte-key"
export DATABASE_URL="/data/symphony.db"
export SYMPHONY_BIN="/usr/local/bin/symphony-platform"
export SYMPHONY_WORKSPACE_ROOT="/data/workspaces"

/usr/local/bin/web-platform
```

### systemd 服务配置

创建 `/etc/systemd/system/symphony.service`：

```ini
[Unit]
Description=Symphony Web Platform
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=symphony
Group=symphony
WorkingDirectory=/opt/symphony
ExecStart=/usr/local/bin/web-platform
Restart=on-failure
RestartSec=5s

# 环境变量（生产环境建议使用 EnvironmentFile）
EnvironmentFile=/etc/symphony/env
Environment=RUST_LOG=web_platform=info

# 资源限制
LimitNOFILE=65536

# 安全加固
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

`/etc/symphony/env` 文件（权限设为 600）：

```
JWT_SECRET=your-production-secret-key-here
ENCRYPTION_KEY=your-base64-encoded-32-byte-key
DATABASE_URL=/data/symphony.db
SYMPHONY_BIN=/usr/local/bin/symphony-platform
SYMPHONY_WORKSPACE_ROOT=/data/workspaces
```

启用并启动：

```bash
systemctl daemon-reload
systemctl enable symphony
systemctl start symphony
systemctl status symphony
```

---

## 前端部署

### 方式一：Nginx 反向代理（推荐）

将前端静态文件和后端 API 统一通过 Nginx 对外暴露：

```nginx
server {
    listen 80;
    server_name your-domain.com;

    # 前端静态文件
    root /opt/symphony/web-frontend/dist;
    index index.html;

    # SPA 路由支持
    location / {
        try_files $uri $uri/ /index.html;
    }

    # 后端 API 反向代理
    location /api/ {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE 支持（AI 流式响应）
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 300s;
    }

    # Swagger UI
    location /swagger-ui {
        proxy_pass http://127.0.0.1:3000;
    }
}
```

### 方式二：静态文件托管

将 `web-frontend/dist/` 上传至任意静态文件托管服务（Nginx、CDN 等），并配置前端的 API 基础 URL 指向后端地址。

---

## 优雅关闭

web-platform 监听 `SIGINT`（Ctrl+C）和 `SIGTERM` 信号，收到信号后执行优雅关闭流程：

1. 停止接受新的 HTTP 请求
2. 向所有受管的 rust-platform 子进程发送停止信号
3. 等待进行中的请求完成
4. 关闭数据库连接池

rust-platform 收到关闭信号后：

1. 设置 `shutting_down` 标志，停止新任务调度
2. 取消所有重试定时器
3. 向所有运行中的 worker 发送 `CancellationToken`（触发 `after_run` hook）
4. 等待 worker 退出（drain timeout 内）
5. 超时后强制 abort 剩余 worker

---

## 监控

### 日志格式

生产环境建议使用 JSON 格式日志，便于日志聚合系统（ELK、Loki 等）解析：

```bash
export RUST_LOG="web_platform=info"
# tracing-subscriber 默认输出文本格式
# 如需 JSON 格式，需在代码中配置 tracing-subscriber 的 json() 格式化器
```

### 健康检查

rust-platform 的 `/api/v1/state` 端点返回当前运行状态快照，可用于监控告警：

```bash
curl http://localhost:<rust_platform_port>/api/v1/state
```

响应示例：

```json
{
  "generated_at": "2026-05-25T10:00:00Z",
  "counts": { "running": 3, "retrying": 1 },
  "running": [...],
  "retrying": [...],
  "codex_totals": { "input_tokens": 12345, "output_tokens": 6789, ... }
}
```

### 告警通道

web-platform 支持钉钉告警通道，通过管理界面配置。告警引擎每 30 秒（可通过 `ALERT_EVAL_INTERVAL_SECS` 调整）评估一次告警规则。

---

## 备份

### 数据库备份

SQLite 数据库文件包含所有项目配置、用户信息和服务状态：

```bash
# 在线备份（SQLite WAL 模式安全）
sqlite3 /data/symphony.db ".backup /backup/symphony-$(date +%Y%m%d).db"

# 或直接复制（服务停止时）
cp /data/symphony.db /backup/symphony-$(date +%Y%m%d).db
```

### 工作空间备份

工作空间目录（`SYMPHONY_WORKSPACE_ROOT`）包含各项目的 WORKFLOW.md 和 symphony.log：

```bash
tar -czf /backup/workspaces-$(date +%Y%m%d).tar.gz /data/workspaces/
```

---

## 升级流程

1. **停止服务**

```bash
systemctl stop symphony
```

2. **备份数据库**

```bash
cp /data/symphony.db /backup/symphony-before-upgrade.db
```

3. **替换二进制**

```bash
cp target/release/web-platform /usr/local/bin/web-platform
cp target/release/symphony-platform /usr/local/bin/symphony-platform
```

4. **启动服务**（自动执行数据库迁移）

```bash
systemctl start symphony
systemctl status symphony
```

5. **验证**

```bash
curl http://localhost:3000/api/v1/health
```

---

## 网络代理配置

若部署环境需要通过代理访问外部 API（GitLab、GitHub、Linear 等），设置以下环境变量：

```bash
export http_proxy="http://proxy.example.com:8080"
export https_proxy="http://proxy.example.com:8080"
export all_proxy="socks5://proxy.example.com:1080"
# 大写形式同样支持
export HTTP_PROXY="http://proxy.example.com:8080"
export HTTPS_PROXY="http://proxy.example.com:8080"
```

web-platform 在启动 rust-platform 子进程时会自动将这些代理环境变量注入子进程，无需额外配置。

也可通过管理界面的"网络代理"设置页面配置代理，该配置会加密存储在数据库中，并在启动子进程时动态注入。
