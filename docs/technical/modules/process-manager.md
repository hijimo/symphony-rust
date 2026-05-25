# Process Manager 实现细节

源文件：`web-platform/src/process_manager/`

---

## 职责

Process Manager 负责 web-platform 管理 rust-platform（symphony-platform）子进程的完整生命周期，包括：

- 启动子进程（注入配置、环境变量、代理设置）
- 监控子进程存活状态
- 自动重启崩溃的子进程
- 启动时清理孤儿进程
- 维护进程状态（PID、启动时间、重启次数）

---

## 模块结构

| 文件 | 职责 |
|------|------|
| `mod.rs` | `ProcessManager` 结构体定义，DashMap 状态存储，per-project 互斥锁 |
| `spawn.rs` | `spawn_symphony()` — 构建命令、注入环境变量、启动子进程 |
| `watcher.rs` | `spawn_watcher()` — 后台健康检查循环，崩溃检测与自动重启 |
| `cleanup.rs` | `startup_cleanup()` — 启动时孤儿进程检测与清理 |
| `pid_verify.rs` | `verify_pid()` — 三因子 PID 验证（存活 + 命令名 + 启动时间） |

---

## 启动流程

`spawn_symphony()`（`spawn.rs`）执行以下步骤：

```
1. 从数据库加载项目配置（owner、platform、token 等）
2. 解密平台 Token（AES-GCM）
3. 创建工作空间目录（workspace_root/<project_id>/）
4. 生成或使用项目的 WORKFLOW.md（自定义内容或模板渲染）
5. 写入 WORKFLOW.md 到工作空间目录
6. 构建 Command：
   - 可执行文件：SYMPHONY_BIN
   - 参数：WORKFLOW.md
   - 工作目录：workspace_dir
   - 环境变量：RUST_LOG、SYMPHONY_SERVICE_INSTANCE_ID、平台 Token、代理变量
7. 设置 stdio：
   - stdin: Stdio::null()
   - stdout: Stdio::from(log_file)   → symphony.log
   - stderr: Stdio::from(log_file)   → symphony.log（同一文件）
8. Unix: setsid() 创建新进程会话（与 web-platform 进程组隔离）
9. spawn() 启动子进程
10. 记录 PID 和 SpawnResult
```

---

## 环境变量注入

启动子进程时注入以下环境变量：

```rust
cmd.env("RUST_LOG", "info");
cmd.env("SYMPHONY_SERVICE_INSTANCE_ID", service_instance_id);

// 平台 Token（解密后明文注入）
cmd.env("GITLAB_TOKEN", &token);   // GitLab 项目
cmd.env("GITHUB_TOKEN", &token);   // GitHub 项目

// GitLab 私有部署主机
cmd.env("GITLAB_HOST", host);

// 代理配置（从数据库加载有效代理配置后注入）
proxy_config.apply_to_command(&mut cmd);
// 注入：http_proxy, https_proxy, all_proxy 及大写形式
```

---

## ProcessManager 数据结构

```rust
pub struct ProcessManager {
    // project_id -> 当前进程状态（DashMap 支持无锁并发读）
    pub processes: Arc<DashMap<i64, ProcessState>>,
    // per-project 互斥锁，序列化同一项目的 start/stop 操作
    pub locks: Arc<DashMap<i64, Arc<Mutex<()>>>>,
}

pub struct ProcessState {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub status: ServiceStatus,    // Running | Stopped | Error | Failed | Starting
    pub restart_count: u32,
}
```

`DashMap` 提供无锁并发读，适合高频状态查询。per-project `Mutex` 确保同一项目的启动/停止操作不会并发执行。

---

## Lease 机制

数据库中的 `projects` 表记录服务实例的 lease 信息，用于跨重启的状态追踪：

| 字段 | 说明 |
|------|------|
| `web_instance_id` | web-platform 实例标识（`SYMPHONY_WEB_INSTANCE_ID` 或 `web-<pid>`） |
| `lifecycle_op_id` | 每次生命周期操作的唯一 UUID |
| `service_owner_web_instance_id` | 拥有该服务实例的 web-platform 实例 |
| `service_generation` | 服务版本号，每次重启递增 |
| `service_instance_id` | 服务实例唯一标识（`svc-<project_id>-<generation>-<uuid>`） |
| `service_pgid` | 进程组 ID（Unix） |
| `service_session_id` | 进程会话 ID（Unix） |
| `service_cmdline_hash` | 命令行哈希（用于验证进程身份） |
| `service_workdir` | 工作目录 |
| `service_proxy_config_version` | 启动时使用的代理配置版本 |

---

## 心跳监控

`spawn_watcher()`（`watcher.rs`）在子进程启动后立即启动后台监控任务：

```
每 10 秒（HEALTH_CHECK_INTERVAL）：
    1. 从 ProcessManager 获取当前状态
    2. 若状态不是 Running，退出监控循环
    3. 调用 pid_verify::verify_pid(pid, started_at)
    4. 若验证通过（进程存活且是 symphony 进程）→ 继续
    5. 若验证失败（进程已死）：
       a. 读取 symphony.log 最后 20 行（用于错误报告）
       b. 检查 auto_restart 配置和 restart_count
       c. 若达到上限（MAX_RESTART_ATTEMPTS = 3）或禁用自动重启：
          → 标记为 Failed，退出监控
       d. 否则：
          → 标记为 Error
          → 等待退避延迟（5s / 15s / 60s）
          → 重新 spawn_symphony()
          → 更新 PID 和 service_generation
          → 继续监控循环
```

---

## 孤儿进程检测

`startup_cleanup()`（`cleanup.rs`）在 web-platform 启动时执行：

```
1. 查询数据库中所有 status = "running" 或 "starting" 的项目
2. 对每个项目：
   a. 若无 PID 记录 → 直接标记为 Stopped
   b. 调用 pid_verify::verify_pid(pid, start_time)
   c. 若进程存活（孤儿进程）：
      → 发送 SIGTERM
      → 等待 2 秒
      → 若仍存活 → 发送 SIGKILL
   d. 标记项目状态为 Stopped
```

---

## PID 验证（pid_verify.rs）

三因子验证确保 PID 对应的是预期的 symphony 进程：

1. **进程存活**：`kill(pid, 0)` 检查进程是否存在（不发送实际信号）
2. **命令名验证**：
   - macOS：`ps -p <pid> -o comm=` 检查命令名包含 "symphony"
   - Linux：读取 `/proc/<pid>/cmdline` 检查包含 "symphony"
3. **启动时间**（最佳努力）：目前 macOS 上依赖因子 1+2，Linux 可扩展

```rust
pub fn verify_pid(pid: u32, _expected_start_secs: i64) -> bool {
    process_exists(pid) && process_is_symphony(pid)
}
```

---

## 异常恢复

进程崩溃后的恢复流程：

```
进程崩溃
    │
    ▼
watcher 检测（10s 间隔）
    │
    ▼
读取 symphony.log 最后 20 行
    │
    ├── restart_count >= 3 或 auto_restart = false
    │       → 标记 Failed，停止监控
    │
    └── 可以重启
            → 标记 Error
            → 等待退避（5s/15s/60s）
            → spawn_symphony()（新 service_generation）
            → 标记 Running，继续监控
```

重启退避时间：

| 重启次数 | 等待时间 |
|----------|----------|
| 第 1 次 | 5 秒 |
| 第 2 次 | 15 秒 |
| 第 3 次及以上 | 60 秒 |

---

## stderr 处理说明

当前实现中，symphony-platform 子进程的 stdout 和 stderr 均重定向到工作空间目录下的 `symphony.log` 文件（`Stdio::from(log_file)`），而非 `Stdio::piped()`。

这是有意为之的设计：

- 避免 pipe buffer 满导致子进程阻塞（deadlock 风险）
- 日志持久化到文件，便于事后排查
- 无需后台任务消费 stderr

若未来需要实时捕获子进程输出，**必须**同时 spawn 后台任务 drain stderr，否则当 stderr 输出超过 pipe buffer（约 64KB）时会导致子进程死锁。
