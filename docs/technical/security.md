# 安全机制

## 认证

### JWT 签发

web-platform 使用 [`jsonwebtoken`](https://crates.io/crates/jsonwebtoken) crate 签发和验证 JWT。

**Token 结构（Claims）**

```json
{
  "sub": "1",          // 用户 ID（字符串）
  "role": "admin",     // 用户角色：admin 或 user
  "exp": 1748000000    // 过期时间（Unix 时间戳）
}
```

**签名算法**：HS256（HMAC-SHA256），密钥来自 `JWT_SECRET` 环境变量。

**过期策略**：Token 有固定过期时间（默认 24 小时）。过期后前端自动登出，用户需重新登录获取新 Token。Token 黑名单（`token_blacklist`）在内存中维护，用于支持主动登出（invalidate）。

---

## 密码安全

用户密码使用 [Argon2](https://crates.io/crates/argon2) 算法哈希后存储，不可逆。

- 算法：Argon2id（抗 GPU 暴力破解）
- 存储：仅存储哈希值，原始密码不落库
- 验证：登录时对输入密码重新哈希并与存储值比对

```rust
// 密码哈希（注册/修改密码时）
let hash = hash_password(&plain_password)?;

// 密码验证（登录时）
verify_password(&plain_password, &stored_hash)?;
```

---

## 数据加密

### 加密算法

敏感数据（API Token、平台凭据、通知渠道配置等）使用 **AES-GCM 256-bit** 对称加密后存储在数据库中。

- 算法：AES-256-GCM（认证加密，防篡改）
- 密钥：来自 `ENCRYPTION_KEY` 环境变量（Base64 编码的 32 字节随机密钥）
- Nonce：每次加密随机生成 12 字节 nonce，与密文一起存储（`nonce || ciphertext`，Base64 编码）

### 加密范围

以下数据在写入数据库前加密：

- 用户配置的 GitLab Token
- 用户配置的 GitHub Token
- 通知渠道配置（Webhook URL、签名密钥等）
- 网络代理配置（含认证信息）

### 密钥管理

`ENCRYPTION_KEY` 是系统安全的核心，需妥善保管：

- 生产环境使用密钥管理服务（KMS、Vault 等）注入
- 不得提交到代码仓库
- 轮换密钥需重新加密所有存储的密文

---

## 授权模型

### 角色定义

| 角色 | 权限范围 |
|------|----------|
| `admin` | 全局管理权限：管理所有用户、所有项目、系统配置 |
| `user` | 项目级权限：仅能访问自己创建或被授权的项目 |

### 项目访问控制

项目成员关系通过 `project_members` 表控制。用户只能访问：

1. 自己创建的项目（`created_by = user_id`）
2. 被显式添加为成员的项目（`project_members` 表中有记录）

admin 用户可访问所有项目。

---

## 中间件链

每个需要认证的 API 请求经过以下中间件链：

```
HTTP 请求
    │
    ▼
jwt_auth          — 从 Authorization: Bearer <token> 提取并验证 JWT
    │               验证签名、检查过期、查询 token 黑名单
    ▼
require_admin     — （仅管理员接口）检查 claims.role == "admin"
    │
    ▼
project_access    — （项目相关接口）检查用户是否为项目成员或 admin
    │
    ▼
Handler           — 业务逻辑处理
```

中间件实现位于：

- `web-platform/src/middleware/jwt.rs` — JWT 验证
- `web-platform/src/middleware/project_access.rs` — 项目访问检查
- `web-platform/src/auth/middleware.rs` — 角色检查

---

## 安全边界

### 服务隔离

- **web-platform** 是唯一对外暴露的服务，监听 `SERVER_PORT`（默认 3000）
- **rust-platform** 作为子进程运行，不直接对外暴露端口（其 HTTP server 仅用于内部状态查询，可选）
- 两者之间通过文件系统（WORKFLOW.md、工作空间目录）和进程管理 API 通信

### 工作空间路径校验

rust-platform 在操作工作空间目录时进行路径校验，防止目录遍历攻击（path traversal）。所有工作空间路径必须在配置的 `workspace.root` 目录下。

### 子进程隔离

rust-platform 子进程通过 `setsid()` 创建新的进程会话，与 web-platform 进程组隔离，避免信号意外传播。

---

## Rate Limiting

### 登录接口限流

登录接口（`POST /api/v1/auth/login`）启用了速率限制，防止暴力破解：

- 基于 IP 地址限流
- 超过阈值后返回 `429 Too Many Requests`
- 实现：`web-platform/src/auth/rate_limit.rs`

### AI 生成接口限流

AI Issue 生成接口有双层限流：

- 每用户每分钟请求数（`AI_RATE_LIMIT_PER_MINUTE`，默认 10）
- 全局每分钟请求数（`AI_GLOBAL_RATE_LIMIT_PER_MINUTE`，默认 30）

---

## 安全配置建议

### HTTPS 部署

生产环境必须通过 HTTPS 提供服务，防止 JWT Token 和 API 密钥在传输中泄露。推荐使用 Nginx + Let's Encrypt：

```bash
certbot --nginx -d your-domain.com
```

### 强密码策略

- `JWT_SECRET`：至少 32 字符，使用随机生成的高熵字符串
- `ENCRYPTION_KEY`：使用 `openssl rand -base64 32` 生成
- admin 初始密码：首次登录后立即修改

### 定期轮换 JWT_SECRET

轮换 `JWT_SECRET` 会使所有已签发的 Token 立即失效，所有用户需重新登录。建议：

- 定期（如每季度）轮换
- 发生安全事件后立即轮换
- 轮换前通知用户

### 数据库文件权限

```bash
chmod 600 /data/symphony.db
chown symphony:symphony /data/symphony.db
```

### 环境变量文件权限

```bash
chmod 600 /etc/symphony/env
chown root:root /etc/symphony/env
```
