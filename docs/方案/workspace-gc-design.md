# Workspace GC 后台任务设计方案 (v4 — Final)

## 背景与问题

每个 issue 在处理时会创建独立的 workspace（浅克隆 + 构建产物），单个 workspace 占用 50MB-1GB+。当前仅在 reconciler 检测到 terminal state **且** worker 恰好还在运行时才清理。大量 workspace 在 PR/MR 合并后成为孤儿，永久占用磁盘。

## 对抗验证历程

| 轮次 | 结果 | 核心修复 |
|------|------|----------|
| v1 | 3x FAIL | 消除 Tracker 依赖；引入 flock 互斥；加批量限制 |
| v2 | 3x FAIL | 统一为单删除路径；lock 文件永不删除；poison 处理 |
| v3 | 3x FAIL | 修复 sidecar 生命周期；修复 run_hook 死锁；加 cycle 时间预算 |

## 核心设计原则

1. **GC 是唯一的 workspace 删除执行者** — reconciler 仅写 terminal marker
2. **本地元数据驱动** — 不依赖 Tracker API，纯文件系统操作
3. **Flock 是唯一的互斥机制** — Worker 和 GC 共享同一 lock path
4. **Lock 文件永不删除** — 防止 inode 漂移破坏互斥
5. **Lock sidecar 在 worker 退出时删除** — 确保 7 天兜底可触发

## 架构

```
┌─────────────────────────────────────────────────────────────┐
│                         main.rs                              │
│                                                              │
│  ┌──────────────────┐         ┌───────────────────────────┐ │
│  │   Orchestrator    │         │   Workspace GC Task        │ │
│  │                   │         │   (sleep-between-cycles)   │ │
│  │ TerminateAndClean:│         │                            │ │
│  │   write marker    │         │   唯一删除执行者：          │ │
│  │                   │         │   1. scan markers          │ │
│  │ TerminateNoClean: │         │   2. acquire flock (NB)    │ │
│  │   (no marker)     │         │   3. run hook + delete     │ │
│  │                   │         │   4. clean quarantine      │ │
│  │ WorkerExit:       │         │   5. stale scan (7d)       │ │
│  │   delete sidecar  │         │                            │ │
│  │   if terminal →   │         │                            │ │
│  │     write marker  │         │                            │ │
│  └──────────────────┘         └───────────────────────────┘ │
│           │                              │                   │
│           ▼                              ▼                   │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Arc<WorkspaceManager>  +  CancellationToken             │ │
│  │  Arc<ConfigHolder> (for hot-reload)                      │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## 设计决策

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 删除执行者 | 仅 GC | 消除双删竞态 |
| 状态来源 | 本地 `.terminal` marker | 消除 Tracker 依赖 |
| 互斥 | 共享 `.symphony/locks/issues/<key>.lock` flock | Worker/GC 同一 lock path |
| Lock 文件 | 永不删除 | 防止 inode 漂移 |
| Lock sidecar `.json` | Worker 退出时删除 | 确保 7 天兜底可触发 |
| GC 间隔 | 可配置 `gc_interval_ms`，默认 300000 (5min)，0=禁用 | ServiceConfig |
| Grace period | 可配置 `gc_retention_ms`，默认 3600000 (1h) | 基于 marker `terminal_since` |
| 每轮上限 | 可配置 `gc_batch_size`，默认 10 | 防止 hook 风暴 |
| Cycle 时间预算 | `gc_cycle_timeout_ms`，默认 120000 (2min) | 防止 hook 超时拖垮 GC |
| Overlap 保护 | sleep-between-cycles | 结构性防止重叠 |
| Poison workspace | gc_attempts >= 3 跳过；24h 后自动重置 | 防止活性丧失 + 允许恢复 |
| 7 天兜底条件 | 无 marker + 无 sidecar `.json` + last_used_at > 7d + flock(NB) 成功 | 防止误标活跃 issue |
| stderr 死锁 | 修复 `run_hook`：drain stderr 或 Stdio::null() | 前置修复，GC 依赖 |

## 前置修复：`run_hook` stderr 管道死锁

**这是已知 bug（CLAUDE.md 已记录），必须在 GC 实现前修复。**

当前代码（`workspace/mod.rs:459-499`）将 stderr 设为 `Stdio::piped()` 但不 drain，子进程 stderr 超过 64KB 时死锁直到 60s 超时。

修复方案：

```rust
// workspace/mod.rs run_hook 方法内
let mut child = command.spawn()...;

let stdout = child.stdout.take();
let stderr = child.stderr.take();

// 后台 drain stdout/stderr 防止管道阻塞
let drain_stdout = tokio::spawn(async move {
    if let Some(mut out) = stdout {
        tokio::io::copy(&mut out, &mut tokio::io::sink()).await.ok();
    }
});
let drain_stderr = tokio::spawn(async move {
    if let Some(mut err) = stderr {
        tokio::io::copy(&mut err, &mut tokio::io::sink()).await.ok();
    }
});

tokio::select! {
    result = child.wait() => { ... }
    _ = tokio::time::sleep(timeout) => { ... }
}

// 清理 drain 任务
drain_stdout.abort();
drain_stderr.abort();
```

## Terminal Marker 机制

### 文件位置

```
<workspace_root>/.symphony/gc/terminal/<issue_id_path_key>.json
```

### 结构

```rust
#[derive(Serialize, Deserialize)]
pub(crate) struct TerminalMarker {
    pub issue_id: String,
    pub issue_id_path_key: String,
    pub workspace_key: String,
    pub terminal_since: DateTime<Utc>,
    pub state: String,
    pub gc_attempts: u32,
    pub last_attempt_at: Option<DateTime<Utc>>,
}
```

### 写入时机

| 场景 | 动作 |
|------|------|
| Reconciler `TerminateAndClean` | 写 marker（不删除 workspace） |
| Worker 正常退出 + issue terminal | 写 marker |
| Worker 正常退出 + issue non-active non-terminal | 不写 marker（7 天兜底处理） |
| Worker 正常退出 + issue still active | 不写 marker（continuation retry） |

## Lock Sidecar 生命周期（v4 关键修复）

**问题**：v3 中 sidecar `.json` 在 worker 退出后永不删除，导致 7 天兜底条件永远不满足。

**修复**：Worker 退出时（无论正常/异常），orchestrator 删除 sidecar `.json`。

```rust
// orchestrator on_worker_exit (正常/异常) 中：
fn cleanup_worker_sidecar(workspace_mgr: &WorkspaceManager, issue_id: &str) {
    let key = WorkspaceManager::issue_id_path_key(issue_id);
    let sidecar_path = workspace_mgr.root()
        .join(".symphony/locks/issues")
        .join(format!("{key}.json"));
    if sidecar_path.exists() {
        let _ = std::fs::remove_file(&sidecar_path);
    }
}
```

**生命周期总结**：

| 文件 | 创建时机 | 删除时机 |
|------|----------|----------|
| `.lock` | `prepare_issue_workspace` 首次调用 | **永不删除** |
| `.json` sidecar | `prepare_issue_workspace` 每次调用 | Worker 退出时（orchestrator 清理） |
| terminal marker | Reconciler/Worker exit 写入 | GC 删除 workspace 后 |

## GC 单次执行流程

```
0. 从 ConfigHolder 快照当前配置
   启动 cycle 计时器 (gc_cycle_timeout_ms)
       │
1. 扫描 .symphony/gc/terminal/ 目录，读取所有 TerminalMarker
       │
2. 分类处理：
       │
       ├─ 2a. workspace 目录不存在 → 删除 marker（孤儿清理）
       │       安全检查：re-read marker 确认 terminal_since 未变
       │
       ├─ 2b. gc_attempts >= 3 且 last_attempt_at < 24h前 → 跳过（poison）
       │       gc_attempts >= 3 且 last_attempt_at >= 24h前 → 重置 attempts，重新尝试
       │
       └─ 2c. terminal_since + retention < now → 加入待清理列表
       │
3. 按 terminal_since 排序（最老优先），取前 batch_size 个
       │
4. 对每个待清理 workspace（检查 cycle 时间预算）：
       │
       ├─ 4a. spawn_blocking: flock(LOCK_EX|LOCK_NB) on .symphony/locks/issues/<key>.lock
       │       失败(EWOULDBLOCK) → 跳过
       │
       ├─ 4b. 【持有锁】运行 before_remove hook
       │       hook 失败 → gc_attempts += 1, last_attempt_at = now
       │                    re-read marker, 更新并写回, 释放锁, 跳过
       │
       ├─ 4c. 【持有锁】remove_dir_all 删除 workspace 目录
       │
       ├─ 4d. 【持有锁】删除 terminal marker 文件
       │
       ├─ 4e. 释放锁（drop File guard）
       │       注意：不删除 .lock 文件，不删除 sidecar（已在 worker 退出时删除）
       │
       └─ 检查 cycle 时间预算，超时则 break
       │
5. 扫描 .symphony/quarantine/ 目录
       │  读取 .quarantined_at 标记文件
       │  超过 retention_period → remove_dir_all（无 hook）
       │  每轮最多 batch_size 个
       │
6. 兜底扫描：workspace root 中 `i-` 前缀目录（跳过 .symphony/ 等隐藏目录）
       │  条件全部满足才标记：
       │    - 无 terminal marker 文件
       │    - 无 lock sidecar .json 文件
       │    - metadata 中 last_used_at（或 initialized_at）距今超过 7 天
       │    - flock(LOCK_NB) 成功获取（确认无 worker 使用中），获取后立即释放
       │  → 写入 terminal marker（下轮 GC 在 grace period 后清理）
       │  每轮最多 batch_size 个
       │
7. Emit 结构化日志 + 条件告警
       │
8. sleep(gc_interval) → 下一轮
```

## Cycle 时间预算

```rust
const DEFAULT_CYCLE_TIMEOUT: Duration = Duration::from_secs(120);

// 在 step 4 循环中：
for workspace in batch {
    if cycle_start.elapsed() > cycle_timeout {
        tracing::warn!(
            processed = processed_count,
            remaining = batch.len() - processed_count,
            "workspace GC: cycle timeout reached, deferring remaining to next cycle"
        );
        break;
    }
    // ... 执行删除
}
```

## Poison Workspace 处理（含自动恢复）

```rust
// Step 2b 检查：
if marker.gc_attempts >= MAX_GC_ATTEMPTS {
    if let Some(last) = marker.last_attempt_at {
        if Utc::now() - last > Duration::hours(24) {
            // 24h 后自动重置，允许重试（hook 可能已修复）
            marker.gc_attempts = 0;
            marker.last_attempt_at = None;
            write_terminal_marker(&marker).await;
            // 本轮不处理，下轮重试
        }
    }
    poison_count += 1;
    continue;
}
```

## 可观测性

```rust
// 每轮结束
tracing::info!(
    scanned = total_markers,
    deleted = deleted_count,
    orphan_markers_cleaned = orphan_count,
    skipped_locked = locked_count,
    skipped_grace = grace_count,
    skipped_hook_fail = hook_fail_count,
    skipped_poison = poison_count,
    quarantine_cleaned = quarantine_count,
    stale_marked = stale_count,
    cycle_timeout_hit = timeout_hit,
    duration_ms = elapsed.as_millis() as u64,
    "workspace GC cycle complete"
);

// 零进度告警（区分原因）
if deleted_count == 0 && total_markers > grace_count {
    if poison_count > 0 && poison_count >= total_markers - grace_count {
        tracing::error!(
            poison = poison_count,
            "workspace GC: all eligible workspaces are poisoned, manual intervention needed"
        );
    } else {
        tracing::warn!(
            pending = total_markers - grace_count,
            locked = locked_count,
            hook_fail = hook_fail_count,
            "workspace GC: cycle completed with no deletions"
        );
    }
}
```

## Reconciler 变更

### TerminateAndClean（改为仅写 marker）

```rust
ReconcileAction::TerminateAndClean { issue_id, identifier } => {
    if let Some(entry) = self.state.running.get(&issue_id) {
        entry.cancel_token.cancel();
    }
    if let Some(entry) = self.state.running.remove(&issue_id) {
        self.state.codex_totals.add_runtime(entry.started_at);
        release_claim(&mut self.state, &issue_id);
        // 写 terminal marker（GC 将在 grace period 后删除）
        if let Some(workspace_mgr) = &self.workspace_mgr {
            workspace_mgr.write_terminal_marker(&issue_id, &identifier, &state).await;
        }
        tracing::info!(issue_id = %issue_id, "reconciler: marked workspace for GC cleanup");
    }
}
```

### Worker 退出时删除 sidecar

```rust
// on_worker_exit_normal 和 on_worker_exit_abnormal 中：
if let Some(workspace_mgr) = &self.workspace_mgr {
    workspace_mgr.delete_lock_sidecar(&issue_id).await;
}
```

## 配置

```yaml
workspace:
  root: ~/code/symphony-workspaces
  gc_interval_ms: 300000        # 5 分钟，0 = 禁用
  gc_retention_ms: 3600000      # 1 小时 grace period
  gc_batch_size: 10             # 每轮最多删除数
  gc_cycle_timeout_ms: 120000   # 每轮最大执行时间
```

## Flock 协议（共享契约）

**不变量：Worker 和 GC 使用完全相同的 lock path**

```
Lock path: <workspace_root>/.symphony/locks/issues/<issue_id_path_key>.lock
```

| 调用者 | 获取方式 | 持有期间 |
|--------|----------|----------|
| Worker | `flock(LOCK_EX)` 阻塞 | 整个 worker 生命周期 |
| GC | `flock(LOCK_EX\|LOCK_NB)` 非阻塞 | hook + 删除期间 |

## 边界情况处理

| 场景 | 处理方式 |
|------|----------|
| Worker 正在使用 | flock(NB) 失败 → 跳过 |
| Reconciler 和 GC 同时处理 | 不可能 — reconciler 不删除 |
| 进程 crash 在 marker 写入后 | GC 下轮正常处理 |
| 进程 crash 在删除中途 | 下轮重试（hook 已执行则 idempotent） |
| Workspace 已删但 marker 残留 | Step 2a 清理孤儿 marker（re-read 验证） |
| Hook 持续失败 | 3 次后 poison，24h 后自动重置重试 |
| Issue 被 tracker 删除 | 7 天兜底（sidecar 已在 worker 退出时删除） |
| TerminateNoClean（非 active 非 terminal） | 不写 marker；sidecar 在 worker 退出时删除；7 天兜底可触发 |
| 长期活跃 issue（retry 间隙） | Step 6 额外要求 flock(NB) 成功 + last_used_at > 7d，防止误标 |
| gc_interval_ms = 0 | GC 任务不启动 |
| Hot-reload terminal_states | 每轮从 ConfigHolder 快照 |
| 非 Unix 平台 | flock no-op，GC 退化为无锁模式（已知限制） |
| Cycle 超时 | break 当前批次，下轮继续 |

### Continuation retry 与 sidecar 的交互

关键场景：worker 退出 → sidecar 删除 → continuation retry 1s 后触发 → `prepare_issue_workspace` 重新创建 sidecar。

在 retry 间隙（~1s），sidecar 不存在。v4 的 7 天兜底通过以下多重保护防止误标：

1. **`last_used_at` 时间戳**：`prepare_issue_workspace` 在复用 workspace 时更新 metadata 中的 `last_used_at` 字段。即使 `initialized_at` 很老，`last_used_at` 是新的（刚被使用过），7 天条件不满足。
2. **flock(LOCK_NB) 检查**：Step 6 在写 marker 前尝试获取 flock。如果 worker 正在运行（持有 flock），获取失败，跳过。
3. **Grace period 缓冲**：即使误写了 marker，还有 1 小时 grace period。在此期间 continuation retry 会重新创建 sidecar，下轮 GC 的 step 4a flock 检查会阻止删除。

### `last_used_at` 机制

在 `prepare_issue_workspace` 的 workspace 复用路径中（metadata 已存在且 init_status == "ready"），更新 metadata：

```rust
// workspace/mod.rs prepare_issue_workspace 复用路径中：
metadata.last_used_at = Some(Utc::now());
write_workspace_metadata(&metadata_path, &metadata).await?;
```

`WorkspaceMetadata` 新增字段：
```rust
struct WorkspaceMetadata {
    // ... existing fields ...
    last_used_at: Option<DateTime<Utc>>,  // 每次复用时更新
}
```

Step 6 的时间判断使用 `last_used_at.unwrap_or(initialized_at)`，确保对旧 metadata（无此字段）向后兼容。

## 实现文件清单

| 文件 | 操作 | 变更内容 |
|------|------|----------|
| `rust-platform/src/workspace/mod.rs` | 修改 | 修复 `run_hook` stderr 死锁；导出子模块；`WorkspaceMetadata` pub(crate) + 新增 `last_used_at` 字段；复用路径更新 `last_used_at`；新增 `write_terminal_marker`、`delete_lock_sidecar` 方法 |
| `rust-platform/src/workspace/gc.rs` | 新建 | GC 任务主逻辑 |
| `rust-platform/src/workspace/terminal_marker.rs` | 新建 | TerminalMarker CRUD |
| `rust-platform/src/orchestrator/mod.rs` | 修改 | TerminateAndClean 改为写 marker；worker exit 时删除 sidecar + 写 terminal marker |
| `rust-platform/src/main.rs` | 修改 | spawn GC 任务 |
| `rust-platform/src/config/service_config.rs` | 修改 | 新增 gc_* 配置字段 |

## 验证方式

1. `cargo build` — 编译通过
2. `cargo clippy` — 无新增 warning
3. **run_hook 修复验证**：测试 hook 输出 >64KB stderr 不死锁
4. 单元测试：terminal marker 写入 → grace period 后 GC 删除
5. 单元测试：grace period 内不删除
6. 单元测试：flock 被持有时跳过
7. 单元测试：batch_size 限制生效
8. 单元测试：cycle timeout 生效
9. 单元测试：孤儿 marker 清理（re-read 验证）
10. 单元测试：poison workspace 跳过 + 24h 自动重置
11. 单元测试：7 天兜底 — 有 sidecar 时不标记，无 sidecar 时标记
12. 单元测试：worker 退出时 sidecar 被删除
13. 单元测试：quarantine 清理
14. 集成验证：reconciler TerminateAndClean 仅写 marker
15. 集成验证：完整 GC 流程端到端
