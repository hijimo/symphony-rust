# Phase 2：项目管理 - 实现方案

## 目标

实现项目管理核心功能，包括 Git URL 解析、项目 CRUD、WORKFLOW.md 配置管理、项目成员管理、Symphony 子进程生命周期管理，以及对应的前端页面。

## 实现范围

### 后端 (web-platform crate)

| 模块 | 内容 |
|------|------|
| Git URL 解析 | 支持 HTTPS/SSH 格式，自动识别 GitLab/GitHub 平台，提取 namespace 和 repo_name |
| 项目 CRUD | 创建/列表/详情/更新/删除项目，基于成员关系的可见性过滤 |
| WORKFLOW.md 管理 | 内置 GitHub/GitLab 默认模板，支持自定义编辑，模板渲染 |
| 项目成员管理 | 手动添加/移除成员，角色管理（owner/member），平台成员同步 |
| 子进程管理 | 启动/停止/重启 Symphony 实例，PID 验证，崩溃恢复，互斥锁 |
| 数据库迁移 | V002__projects.sql 新增 projects、project_members 表 |

### 前端 (web-frontend)

| 模块 | 内容 |
|------|------|
| 项目列表页 | 项目卡片/列表视图，服务状态指示，快速操作 |
| 项目创建 | Git URL 输入，自动解析，平台识别 |
| 项目设置页 | 基本信息编辑，WORKFLOW.md 编辑器，服务控制 |
| 成员管理页 | 成员列表，添加/移除，角色切换，平台同步 |
| 侧边栏更新 | 新增"项目"导航分组 |

### API 接口清单

```
GET    /api/projects                       项目列表（按成员关系过滤）
POST   /api/projects                       创建项目
GET    /api/projects/:id                   项目详情
PUT    /api/projects/:id                   更新项目配置
DELETE /api/projects/:id                   删除项目（owner 权限）

POST   /api/projects/:id/start             启动服务
POST   /api/projects/:id/stop              停止服务
POST   /api/projects/:id/restart           重启服务
GET    /api/projects/:id/status            服务运行状态

GET    /api/projects/:id/members           成员列表
POST   /api/projects/:id/members           添加成员
PUT    /api/projects/:id/members/:user_id  更新成员角色
DELETE /api/projects/:id/members/:user_id  移除成员
POST   /api/projects/:id/members/sync      从平台同步成员

GET    /api/projects/:id/workflow           获取 WORKFLOW.md 内容
PUT    /api/projects/:id/workflow           更新 WORKFLOW.md 内容
POST   /api/projects/:id/workflow/reset     重置为默认模板
```

---

## 技术设计

### 1. Git URL 解析

#### 支持格式

| 格式 | 示例 |
|------|------|
| HTTPS (GitLab) | `https://gitlab.com/group/project` |
| HTTPS (GitLab 多级) | `https://gitlab.com/group/sub/project` |
| HTTPS (GitHub) | `https://github.com/owner/repo` |
| SSH (GitLab) | `git@gitlab.com:group/project.git` |
| SSH (GitHub) | `git@github.com:owner/repo.git` |
| 自建 GitLab | `https://gitlab.example.com/group/project` |

#### 平台识别规则

```rust
pub struct ParsedGitUrl {
    pub platform: Platform,       // GitHub | GitLab
    pub host: String,             // "github.com" | "gitlab.com" | 自定义域名
    pub namespace: String,        // "owner" | "group/sub"
    pub repo_name: String,        // "project"
    pub normalized_url: String,   // 标准化后的 HTTPS URL
}

pub enum Platform {
    GitHub,
    GitLab,
}
```

**识别逻辑**：
1. 域名包含 `github.com` → GitHub
2. 域名包含 `gitlab` → GitLab
3. 其他域名 → 默认 GitLab（自建实例场景）
4. 用户可在创建时手动指定平台（覆盖自动识别）

#### 实现模块

```rust
// web-platform/src/git_url.rs
pub fn parse_git_url(url: &str) -> Result<ParsedGitUrl, GitUrlError>;
```

### 2. 数据库迁移

#### V002__projects.sql

```sql
-- 项目表（Phase 1 已建表，此处为确认）
-- projects 表结构见 Phase 1 迁移

-- 如果 Phase 1 未包含以下字段，需要 ALTER TABLE
ALTER TABLE projects ADD COLUMN max_concurrent_agents INTEGER DEFAULT 2;
ALTER TABLE projects ADD COLUMN auto_restart BOOLEAN DEFAULT true;
ALTER TABLE projects ADD COLUMN restart_count INTEGER DEFAULT 0;
ALTER TABLE projects ADD COLUMN last_started_at TEXT;
ALTER TABLE projects ADD COLUMN last_stopped_at TEXT;
ALTER TABLE projects ADD COLUMN error_message TEXT;
```

### 3. Repository 扩展

```rust
// web-platform/src/repository/mod.rs

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create_project(&self, project: &NewProject) -> Result<Project>;
    async fn get_project(&self, id: i64) -> Result<Option<Project>>;
    async fn list_projects_for_user(&self, user_id: i64, is_admin: bool) -> Result<Vec<Project>>;
    async fn update_project(&self, id: i64, updates: &ProjectUpdate) -> Result<()>;
    async fn delete_project(&self, id: i64) -> Result<()>;
    async fn update_service_status(&self, id: i64, status: &ServiceStatus) -> Result<()>;
}

#[async_trait]
pub trait ProjectMemberRepository: Send + Sync {
    async fn list_members(&self, project_id: i64) -> Result<Vec<ProjectMember>>;
    async fn add_member(&self, project_id: i64, user_id: i64, role: &str) -> Result<()>;
    async fn update_member_role(&self, project_id: i64, user_id: i64, role: &str) -> Result<()>;
    async fn remove_member(&self, project_id: i64, user_id: i64) -> Result<()>;
    async fn is_member(&self, project_id: i64, user_id: i64) -> Result<bool>;
    async fn get_member_role(&self, project_id: i64, user_id: i64) -> Result<Option<String>>;
    async fn sync_members(&self, project_id: i64, members: &[SyncMember]) -> Result<SyncResult>;
}
```

### 4. WORKFLOW.md 模板管理

#### 内置模板

模板文件嵌入二进制（`include_str!`）：

```
web-platform/src/templates/
├── workflow_github.md    # GitHub 项目默认模板
└── workflow_gitlab.md    # GitLab 项目默认模板
```

#### 模板渲染

使用 Liquid 模板引擎渲染项目特定变量：

```rust
struct WorkflowTemplateContext {
    platform: String,           // "github" | "gitlab"
    project_slug: String,       // "namespace/repo_name"
    workspace_root: String,     // ~/symphony-workspaces/{project_id}
    max_concurrent_agents: u32, // 项目并发数
    default_branch: String,     // "main"
}
```

#### 存储策略

- `workflow_template = "default"` → 使用内置模板 + 项目变量渲染
- `workflow_template = "custom"` → 使用 `workflow_content` 字段存储的自定义内容
- 切换回默认时清空 `workflow_content`

### 5. Symphony 子进程生命周期管理

#### 架构

```rust
// web-platform/src/process_manager/mod.rs

pub struct ProcessManager {
    processes: Arc<DashMap<i64, ProcessState>>,  // project_id -> state
    locks: Arc<DashMap<i64, Arc<Mutex<()>>>>,    // per-project mutex
}

pub struct ProcessState {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub status: ServiceStatus,
    pub restart_count: u32,
}

pub enum ServiceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Error(String),
    Failed,  // 超过重试次数
}
```

#### 启动流程

```
POST /api/projects/:id/start
    │
    ├── 1. 获取 per-project Mutex（超时 5s）
    ├── 2. 检查当前状态（已运行则返回冲突错误）
    ├── 3. 从 DB 读取项目配置
    ├── 4. 渲染 WORKFLOW.md → 写入临时文件
    ├── 5. 从 DB 读取 owner 的 Token 并解密
    ├── 6. 构建 Command:
    │       symphony-rust --workflow {path} --port {dynamic_port}
    │       env: GITLAB_TOKEN / GITHUB_TOKEN, GITLAB_HOST
    ├── 7. 使用 setsid 创建新进程组
    ├── 8. 启动子进程，记录 PID + 启动时间
    ├── 9. 更新 DB: service_status = 'running', service_pid = pid
    ├── 10. 启动健康检查 watcher（后台任务）
    └── 11. 释放 Mutex
```

#### 停止流程

```
POST /api/projects/:id/stop
    │
    ├── 1. 获取 per-project Mutex
    ├── 2. PID 验证（三要素检查）
    ├── 3. 发送 SIGTERM
    ├── 4. 等待退出（超时 30s）
    ├── 5. 超时未退出 → SIGKILL
    ├── 6. 更新 DB: service_status = 'stopped'
    └── 7. 释放 Mutex
```

#### PID 验证

```rust
fn verify_pid(pid: u32, started_at: DateTime<Utc>) -> bool {
    // macOS: ps -p <pid> -o comm=
    // Linux: /proc/<pid>/cmdline
    // 1. 进程存在
    // 2. 进程名包含 "symphony-rust"
    // 3. 进程启动时间与记录匹配（容差 2s）
}
```

#### 崩溃恢复

```rust
// 后台 watcher 检测到子进程退出
async fn on_process_exit(project_id: i64, exit_code: i32) {
    if project.auto_restart && project.restart_count < 3 {
        let delay = match project.restart_count {
            0 => Duration::from_secs(5),
            1 => Duration::from_secs(15),
            _ => Duration::from_secs(60),
        };
        sleep(delay).await;
        restart_service(project_id).await;
        project.restart_count += 1;
    } else {
        // 标记为 failed，触发告警
        update_status(project_id, ServiceStatus::Failed).await;
    }
}
```

#### 平台启动清理

```rust
// web-platform 启动时执行
async fn startup_cleanup(db: &SqliteRepository) {
    let running_projects = db.list_projects_by_status("running").await;
    for project in running_projects {
        if !verify_pid(project.service_pid, project.last_started_at) {
            // PID 无效，标记为 stopped
            db.update_service_status(project.id, "stopped").await;
        }
        // PID 有效但属于旧实例 → SIGTERM 清理
    }
}
```

### 6. 项目成员管理

#### 权限模型

| 操作 | owner | member | admin |
|------|-------|--------|-------|
| 查看项目 | ✓ | ✓ | ✓ |
| 启停服务 | ✓ | ✗ | ✓ |
| 修改配置 | ✓ | ✗ | ✓ |
| 管理成员 | ✓ | ✗ | ✓ |
| 删除项目 | ✓ | ✗ | ✓ |
| 看板操作 | ✓ | ✓ | ✓ |

#### 平台同步

```rust
async fn sync_members_from_platform(
    project: &Project,
    token: &str,
) -> Result<SyncResult> {
    // 1. 调用 GitLab/GitHub API 获取项目成员列表
    // 2. 按 username 匹配本系统已注册用户
    // 3. 匹配成功 → 添加为 member（已存在则跳过）
    // 4. 返回结果: matched_count, unmatched_usernames
}

pub struct SyncResult {
    pub added: u32,
    pub skipped: u32,  // 已存在
    pub unmatched: Vec<String>,  // 未注册的平台用户名
}
```

### 7. 权限中间件扩展

```rust
// 项目级权限检查中间件
async fn require_project_access(
    user: &AuthUser,
    project_id: i64,
    required_role: Option<&str>,  // None = 任意成员, Some("owner") = 仅 owner
) -> Result<(), WebPlatformError> {
    if user.role == "admin" {
        return Ok(());  // admin 可访问所有项目
    }
    let member_role = repo.get_member_role(project_id, user.id).await?;
    match (member_role, required_role) {
        (Some(_), None) => Ok(()),
        (Some(role), Some(required)) if role == required => Ok(()),
        _ => Err(WebPlatformError::Forbidden),
    }
}
```

---

## 前端实现

### 页面路由

| 页面 | 路由 | 权限 |
|------|------|------|
| 项目列表 | `/projects` | 所有登录用户 |
| 创建项目 | `/projects/new` | 所有登录用户 |
| 项目设置 | `/projects/:id/settings` | owner / admin |
| 项目成员 | `/projects/:id/members` | owner / admin |

### 侧边栏更新

```typescript
const menuItems = [
  {
    group: '项目',
    roles: ['admin', 'user'],
    items: [
      { path: '/projects', label: '项目列表', icon: FolderOutline },
    ],
  },
  {
    group: '管理',
    roles: ['admin'],
    items: [
      { path: '/admin/users', label: '用户管理', icon: PeopleOutline },
      { path: '/admin/config', label: '系统配置', icon: TuneOutline },
    ],
  },
  {
    group: '个人',
    roles: ['admin', 'user'],
    items: [
      { path: '/settings', label: '个人设置', icon: SettingsOutline },
    ],
  },
];
```

### 状态管理

```typescript
// store/projectStore.ts
interface ProjectStore {
  projects: Project[];
  currentProject: Project | null;
  loading: boolean;
  fetchProjects: () => Promise<void>;
  createProject: (data: CreateProjectData) => Promise<void>;
  startService: (id: number) => Promise<void>;
  stopService: (id: number) => Promise<void>;
}
```

---

## 实现顺序

### 后端

1. Git URL 解析模块 (`git_url.rs`)
2. 数据库迁移 V002
3. ProjectRepository trait + SqliteRepository 实现
4. ProjectMemberRepository trait + SqliteRepository 实现
5. WORKFLOW.md 模板管理（内置模板 + 渲染）
6. ProcessManager 骨架（启动/停止/状态查询）
7. PID 验证 + 启动清理
8. 崩溃恢复 watcher
9. 项目 CRUD handlers
10. 项目成员 handlers
11. 服务控制 handlers（start/stop/restart）
12. 权限中间件（项目级）
13. 平台成员同步
14. OpenAPI 文档更新

### 前端

15. API 层扩展（projects, members）
16. projectStore (Zustand)
17. 项目列表页
18. 创建项目页（含 URL 解析预览）
19. 项目设置页（基本信息 + WORKFLOW.md 编辑器）
20. 项目成员管理页
21. 侧边栏导航更新
22. 服务状态轮询 + 控制按钮

---

## 验收标准

- [ ] `cargo build --workspace` 编译通过
- [ ] `cargo test -p web-platform` 所有测试通过
- [ ] Git URL 解析覆盖所有格式（HTTPS/SSH/自建域名）
- [ ] 创建项目 → 自动识别平台 → 生成 WORKFLOW.md
- [ ] 启动服务 → 子进程运行 → health check 通过
- [ ] 停止服务 → 子进程优雅退出 → 状态更新
- [ ] 崩溃恢复：手动 kill 子进程 → 自动重启（3次内）
- [ ] 平台启动清理：重启 web-platform → 孤儿进程正确处理
- [ ] 成员管理：添加/移除/角色切换正常
- [ ] 权限隔离：普通用户只能看到自己的项目
- [ ] 前端项目列表正确展示服务状态
- [ ] 前端 WORKFLOW.md 编辑器可编辑并保存

---

## 环境变量（新增）

```env
# Phase 2 新增
SYMPHONY_BINARY_PATH=/usr/local/bin/symphony-rust   # Symphony 二进制路径
WORKSPACE_ROOT=~/symphony-workspaces                # 工作空间根目录
SERVICE_HEALTH_CHECK_INTERVAL=10                    # 健康检查间隔（秒）
SERVICE_STOP_TIMEOUT=30                             # 停止超时（秒）
MAX_RESTART_ATTEMPTS=3                              # 最大自动重启次数
```

---

## 目录结构（新增/修改）

```
web-platform/
├── migrations/
│   └── V002__projects_extend.sql
├── src/
│   ├── git_url.rs                    # Git URL 解析
│   ├── process_manager/
│   │   ├── mod.rs                    # ProcessManager 主结构
│   │   ├── pid_verify.rs             # PID 验证
│   │   ├── watcher.rs                # 健康检查 + 崩溃恢复
│   │   └── cleanup.rs                # 启动清理
│   ├── templates/
│   │   ├── mod.rs                    # 模板管理
│   │   ├── workflow_github.md        # GitHub 默认模板
│   │   └── workflow_gitlab.md        # GitLab 默认模板
│   ├── handlers/
│   │   ├── projects.rs               # 项目 CRUD handlers
│   │   ├── project_members.rs        # 成员管理 handlers
│   │   └── project_service.rs        # 服务控制 handlers
│   ├── repository/
│   │   └── sqlite.rs                 # 扩展 ProjectRepository 实现
│   └── middleware/
│       └── project_access.rs         # 项目级权限中间件

web-frontend/
├── src/
│   ├── api/
│   │   ├── projects.ts               # 项目 API
│   │   └── members.ts                # 成员 API
│   ├── store/
│   │   └── projectStore.ts           # 项目状态管理
│   ├── pages/
│   │   ├── projects/
│   │   │   ├── ProjectListPage.tsx   # 项目列表
│   │   │   ├── CreateProjectPage.tsx # 创建项目
│   │   │   ├── ProjectSettingsPage.tsx # 项目设置
│   │   │   └── ProjectMembersPage.tsx  # 成员管理
│   ├── components/
│   │   ├── projects/
│   │   │   ├── ProjectCard.tsx       # 项目卡片
│   │   │   ├── ServiceStatusBadge.tsx # 服务状态指示
│   │   │   ├── GitUrlInput.tsx       # Git URL 输入 + 解析预览
│   │   │   ├── WorkflowEditor.tsx    # WORKFLOW.md 编辑器
│   │   │   ├── MemberTable.tsx       # 成员表格
│   │   │   └── ServiceControlPanel.tsx # 服务控制面板
```

---

## 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 子进程管理跨平台差异 | macOS/Linux 行为不同 | 条件编译 + 充分测试 |
| PID 复用导致误操作 | 信号发送给错误进程 | 三要素验证（PID + cmdline + 时间） |
| 并发启停竞态 | 状态不一致 | per-project Mutex + 状态机 |
| 平台 API 限流 | 成员同步失败 | 重试 + 用户提示 |
| WORKFLOW.md 模板变更 | 已运行服务不受影响 | 重启后生效，文档说明 |
