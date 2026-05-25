# 故障排查

## 启动失败

### 端口占用

**现象**：后端启动时报 `Address already in use (os error 98)`

**排查**：

```bash
# 查看占用 3000 端口的进程
lsof -i :3000
# 或
ss -tlnp | grep 3000
```

**解决**：

```bash
# 终止占用进程
kill -9 <PID>
# 或修改端口
export SERVER_PORT=3001
```

---

### 数据库文件权限

**现象**：启动时报 `unable to open database file` 或 `permission denied`

**排查**：

```bash
ls -la data.db
ls -la $(dirname $DATABASE_URL)
```

**解决**：

```bash
chmod 644 data.db
chown <运行用户> data.db
# 确保目录可写
chmod 755 $(dirname $DATABASE_URL)
```

---

### 缺少必填环境变量

**现象**：启动时 panic，报 `Environment variable JWT_SECRET is required but not set` 或 `Failed to parse ENCRYPTION_KEY`

**解决**：确保以下变量已设置：

```bash
export JWT_SECRET="your-secret-at-least-32-chars"
export ENCRYPTION_KEY="$(openssl rand -base64 32)"
```

开发环境使用 `./dev.sh` 启动可自动设置这些变量。

---

## Agent 停滞

### stall_timeout_ms 配置

Agent 停滞检测由 `codex.stall_timeout_ms` 控制（默认 300000ms = 5分钟）。若 Agent 在该时间内无任何 Codex 事件输出，orchestrator 会：

1. 发送 `CancellationToken` 取消信号（触发 `after_run` hook）
2. 等待 30 秒
3. 若仍未退出，强制 kill

调整停滞超时：

```yaml
codex:
  stall_timeout_ms: 600000   # 延长到 10 分钟
```

---

### stderr pipe buffer 满导致 deadlock

**现象**：Agent 进程无响应，stdout 无输出，进程未退出，最终触发停滞超时

**根本原因**：若子进程的 stderr 设为 `Stdio::piped()` 但没有后台任务消费，当 stderr 输出超过 pipe buffer（约 64KB）时，子进程会阻塞在 `write(stderr)`，导致 stdout 也无法响应。

**当前实现**：rust-platform 的 Codex 子进程 stderr 重定向到日志文件（`Stdio::from(log_file)`），web-platform 的 symphony-platform 子进程同样重定向到 `symphony.log`，均不使用 `Stdio::piped()`，因此不存在此问题。

**排查**：若自定义了 Codex 命令或 Hook 脚本，确保不会产生大量 stderr 输出而无人消费。

---

### 检查运行状态

```bash
# 查看 rust-platform 当前运行状态（需配置 server.port）
curl http://localhost:<port>/api/v1/state | jq .

# 查看 symphony.log（子进程日志）
tail -f <SYMPHONY_WORKSPACE_ROOT>/<project_id>/symphony.log

# 查看 web-platform 日志
export RUST_LOG="web_platform=debug"
```

---

## Token 过期

**现象**：前端请求返回 `401 Unauthorized`，或页面自动跳转到登录页

**原因**：JWT Token 已过期（默认有效期 24 小时）

**解决**：重新登录获取新 Token。前端在收到 401 响应时会自动清除本地 Token 并跳转登录页。

**调整过期时间**：Token 过期时间在代码中配置，如需修改需重新编译。

---

## 数据库锁

**现象**：数据库操作报 `database is locked` 错误

**原因**：SQLite 在 WAL 模式下支持并发读，但写操作仍需独占锁。长事务或多进程同时写入可能导致锁等待超时。

**排查**：

```bash
# 检查是否有多个进程打开了数据库
lsof data.db

# 检查 WAL 文件大小（过大说明有未提交的事务）
ls -lh data.db data.db-shm data.db-wal
```

**解决**：

```bash
# 强制 WAL checkpoint（服务停止时）
sqlite3 data.db "PRAGMA wal_checkpoint(TRUNCATE);"

# 若 WAL 文件损坏，可尝试恢复
sqlite3 data.db ".recover" | sqlite3 data_recovered.db
```

---

## 进程残留

**现象**：web-platform 异常退出后，symphony-platform 子进程仍在运行（孤儿进程）

**原因**：web-platform 崩溃时来不及清理子进程

**自动处理**：web-platform 重启时，`startup_cleanup` 会自动检测数据库中记录为 running/starting 状态的项目，通过 `pid_verify` 确认进程是否存活，并向孤儿进程发送 SIGTERM（等待 2 秒后若未退出则 SIGKILL）。

**手动清理**：

```bash
# 查找残留的 symphony-platform 进程
ps aux | grep symphony-platform

# 终止进程
kill -TERM <PID>
# 若无响应
kill -KILL <PID>
```

---

## Tracker API 错误

### 认证失败

**现象**：日志中出现 `Tracker API returned status 401` 或 `403`

**排查**：

```bash
# 验证 Token 是否有效（以 GitLab 为例）
curl -H "PRIVATE-TOKEN: $GITLAB_TOKEN" https://gitlab.com/api/v4/user
```

**解决**：更新 WORKFLOW.md 中的 `api_key` 或对应的环境变量。

---

### Rate Limit

**现象**：日志中出现 `Tracker API returned status 429`

**解决**：增大 `polling.interval_ms` 减少轮询频率：

```yaml
polling:
  interval_ms: 30000   # 从 5s 增大到 30s
```

---

### 网络代理配置

**现象**：日志中出现连接超时或 `connection refused`，但手动 curl 加代理可以访问

**解决**：确保代理环境变量已设置，并在 web-platform 启动前生效：

```bash
export https_proxy="http://proxy.example.com:8080"
export http_proxy="http://proxy.example.com:8080"
```

也可通过管理界面的"网络代理"页面配置，该配置会自动注入到 rust-platform 子进程。

---

## 工作空间问题

### 磁盘空间不足

**现象**：`after_create` hook 失败，报 `No space left on device`

**排查**：

```bash
df -h <SYMPHONY_WORKSPACE_ROOT>
du -sh <SYMPHONY_WORKSPACE_ROOT>/*/
```

**解决**：

1. 清理已完成项目的工作空间（通过管理界面触发 GC，或手动删除）
2. 扩容磁盘
3. 调整 `workspace.root` 到空间更大的目录

---

### 权限问题

**现象**：工作空间目录创建失败，报 `Permission denied`

**解决**：

```bash
mkdir -p <SYMPHONY_WORKSPACE_ROOT>
chown -R <运行用户> <SYMPHONY_WORKSPACE_ROOT>
chmod 755 <SYMPHONY_WORKSPACE_ROOT>
```

---

### workspace GC

工作空间 GC 在以下情况触发：

- Issue 进入终态后，orchestrator 清理对应工作空间
- 手动通过管理界面触发

若 GC 未正常执行导致工作空间堆积，可手动清理：

```bash
# 查看各工作空间大小
du -sh <SYMPHONY_WORKSPACE_ROOT>/*/

# 手动删除特定工作空间（确认 Issue 已完成）
rm -rf <SYMPHONY_WORKSPACE_ROOT>/<issue_id>/
```

---

## 前端构建失败

### Node 版本不匹配

**现象**：`npm install` 或 `npm run build` 报语法错误或 API 不存在

**解决**：

```bash
node --version   # 确认 >= 18
nvm use 18       # 切换到正确版本
```

---

### 依赖安装失败

**现象**：`npm install` 报网络错误或包不存在

**解决**：

```bash
# 清理缓存重试
npm cache clean --force
rm -rf node_modules package-lock.json
npm install

# 使用镜像源（国内网络）
npm install --registry https://registry.npmmirror.com
```

---

### TypeScript 类型错误

**现象**：`npm run build` 报 TypeScript 类型错误

**排查**：

```bash
cd web-frontend
npx tsc --noEmit   # 仅类型检查，不生成文件
```

常见原因：

- API 响应类型与后端不一致（需同步更新前端类型定义）
- 依赖版本升级导致类型变化

---

## 日志排查方法

### 开启详细日志

```bash
# web-platform 详细日志
export RUST_LOG="web_platform=debug,tower_http=debug"

# rust-platform 详细日志（在 WORKFLOW.md 中或启动时设置）
export RUST_LOG="symphony_platform=debug"

# 特定模块 trace 级别
export RUST_LOG="symphony_platform::orchestrator=trace,symphony_platform::tracker=debug,symphony_platform::agent=debug"
```

### 关键日志模式

| 日志内容 | 含义 |
|----------|------|
| `Orchestrator starting event loop` | rust-platform 正常启动 |
| `spawned agent worker` + `issue_id` | Issue 开始处理 |
| `worker exited normally` | Worker 正常完成一轮 |
| `worker exited abnormally` + `error` | Worker 异常退出，查看 error 字段 |
| `stall detected` | 停滞检测触发 |
| `reconciler Part B: terminated worker` | 协调器终止了终态 Issue 的 worker |
| `configuration reloaded` | WORKFLOW.md 热重载成功 |
| `Failed to parse ENCRYPTION_KEY` | ENCRYPTION_KEY 格式错误 |
| `Process died unexpectedly` | symphony-platform 子进程崩溃 |
| `Orphan process, sending SIGTERM` | 启动清理发现孤儿进程 |

### 查看子进程日志

symphony-platform 的日志写入工作空间目录下的 `symphony.log`：

```bash
# 实时查看
tail -f <SYMPHONY_WORKSPACE_ROOT>/<project_id>/symphony.log

# 查看最后 100 行
tail -100 <SYMPHONY_WORKSPACE_ROOT>/<project_id>/symphony.log
```
