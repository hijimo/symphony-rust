# AI 文档工作台：公共编辑器组件方案（路线 B）

> **范围**：本方案只覆盖**公共编辑器组件**，即网页上的“目录、文件、编辑、预览、AI 对话”工作台。
> intent / spec / plan 的闸门、审批、状态机和触发规则属于**完整流程方案**，后续另定。本文只给流程预留接入点。
>
> **状态**：设计草案，未实现。
>
> **依据**：当前代码结构；`deploy/docker/Dockerfile` 中的 `CODEX_VERSION=0.159.0`；`rust-platform/src/agent/codex_client.rs` 已实现的协议方法；本机 `codex-cli 0.158.0` 生成的 app-server schema。部署机 0.159.0 的协议尚未核对。

## 1. 目标体验

在网页上做到本地 Codex、VS Code、Kiro 的核心体验：

1. 浏览主仓库的目录结构，打开任意文件查看，支持多标签和搜索。
2. 编辑 Markdown 时，实时预览渲染效果。
3. 和 AI 多轮对话。AI 能读完整代码、追问、改文件；改动实时出现在编辑区和预览里，可以审阅、接受或回退。
4. 可以引用上下文：`@文件`，或者选中一段让 AI 修改。
5. AI 遵守和本地一致的仓库级、组织级规则与 skills。

**明确不做**：终端、调试、扩展市场、多人实时协同编辑（CRDT）、完整的 Git 图形界面。

## 2. 组件定位与边界

工作台是一个**与业务流程无关**的组件。流程方案通过参数和回调来使用它。

**调用方传入（WorkbenchContext）：**

| 字段 | 说明 |
|---|---|
| `projectId` | 项目 ID |
| `workspaceKey` | 工作区标识，例如 issue iid |
| `branch` | 工作区分支 |
| `writablePaths` | 可写路径白名单，例如 `.symphony/changes/123/**` |
| `readonlyPaths` | 只读范围，默认整个主仓库 |
| `initialFiles` | 默认打开的文件 |
| `promptPresets` | 快捷指令，例如“生成 spec”，由流程方案提供 |
| `agentPolicy` | 模型、sandbox、审批策略 |

**组件对外提供：**

- 回调：`onCommitted(sha)`、`onTurnCompleted(diff)`。
- 插槽：工具栏 actions slot，流程方案往里放“提交审批”之类的按钮。

组件不知道什么是 intent、spec 或审批，这些全部由调用方决定。

## 3. 前端设计

### 3.1 布局

```text
┌───────────┬──────────────────────────────┬───────────────┐
│ 文件目录   │ [intent.md][spec.md][router.rs]  │ AI 对话          │
│           │ ┌────────────┬─────────────┐ │               │
│ ▸ src/    │ │ 源码编辑    │ 实时预览     │ │ 消息流 / 过程卡片 │
│ ▸ docs/   │ │ (Monaco)   │ (Markdown/  │ │ 追问 / 审批卡片   │
│ ▾ .symphony│ │            │  Mermaid)   │ │               │
│   spec.md ●│ └────────────┴─────────────┘ │ @文件 引用      │
│           │ [AI 改动：接受 / 回退 / 查看 diff] │ [输入框]        │
├───────────┴──────────────────────────────┴───────────────┤
│ 状态：分支 · 主干 SHA · 上下文完整性 · 未保存数 · AI 状态 · [流程插槽] │
└──────────────────────────────────────────────────────────┘
```

三栏可拖拽（`react-resizable-panels`）：

- **左栏**：目录树和搜索。
- **中栏**：多标签编辑器。Markdown 文件可以切换“源码、分栏、预览”三种视图。
- **右栏**：AI 对话面板。
- **底栏**：状态信息和流程插槽。

### 3.2 技术选型

| 部分 | 选型 | 理由和取舍 |
|---|---|---|
| 编辑器 | Monaco | 体验最接近 VS Code，自带多 model 标签、DiffEditor、装饰和行内 widget。包体积大，按路由懒加载。CodeMirror 6 更轻，但要自己拼 diff 和多标签 |
| 预览 | react-markdown + remark-gfm + rehype-highlight + mermaid | 支持 GFM 表格、代码高亮和流程图。渲染时给块级元素注入源码行号，用于同步滚动和点预览跳回源码 |
| 目录树 | react-arborist 或 MUI X TreeView | 支持懒加载和虚拟滚动 |
| 状态 | zustand（项目现有） | 拆成 `workspaceStore`（树、标签、buffer、版本）和 `agentStore`（会话、消息、turn、待审批） |

### 3.3 交互细节

- **未保存与冲突**：每个 buffer 记录 `baseVersion`（打开时的内容哈希）。文件被 AI 或其他人改了：
  - buffer 没有改动，就静默刷新；
  - buffer 有改动，就弹出冲突条，提供“查看 diff、保留我的、采用新的”三个选项。
- **AI 改动可视化**：
  - 编辑器 gutter 色块标出本轮 AI 改动的行，每块可以单独“接受”或“回退”。
  - 目录树里被改的文件带圆点标记。
  - 对话面板列出本轮改动的文件，点击打开 DiffEditor。
- **上下文引用**：
  - 输入框输入 `@` 触发文件模糊搜索。
  - 编辑器里选中文本后，右键或浮动按钮选“让 AI 修改这段”，自动带上路径、行号和选中文本。
  - 对话消息中的文件路径可点击，打开文件并跳到对应行。
- **AI 过程透明**：
  - 消息流式显示；读文件、执行命令、改文件以折叠卡片展示。
  - AI 追问时显示问题卡片，用户回答后继续。
  - 需要审批时显示“允许、拒绝”卡片。
  - 任何时候可以“停止”，也可以在运行中“追加说明”。
- **权限表现**：不在 `writablePaths` 内的文件以只读打开，标签带锁图标。
- **上下文提示**：主干有更新、子模块或 LFS 缺失时，在顶部显示提示条。

## 4. 后端设计（web-platform）

新增两个服务，使用同一个工作区。

### 4.1 WorkspaceService：文件能力

#### 工作区结构

```text
/workspaces/editor/
├── mirrors/<project>.git            # 每个项目一份 bare mirror，保留完整历史
└── <project>/<workspaceKey>/        # git worktree，完整检出
    ├── <主仓库全部被跟踪的文件>
    └── repos/<alias>/               # 多仓库项目：从仓库只读检出（已确定）
```

- **完整代码**：worktree 检出指定提交下所有被 Git 跟踪的文件，不使用 `sparse-checkout`。多仓库项目的文档放主仓库。
- **从仓库**（已确定）：多仓库项目的每个从仓库，同样以“bare mirror + worktree”完整检出到 `repos/<alias>/`，完整代码和完整历史。`repos/**` 不在 `writablePaths` 内，AI 写入被自动拒绝，提交只包含主仓库文档目录。`<alias>` 与项目多仓库配置（`docs/方案/multi-repo-workspace-design.md`）一致。主仓库内需要把 `repos/` 视为工作区外的目录：不进入主仓库的 `git status`，目录树中单独分组显示。
- **完整历史**：mirror 默认保留全部历史，不加 `--depth`，Codex 可正常使用 `git log`、`git blame`、`git show`。
- **大仓库可选项**：项目配置可给 mirror 开启 `--filter=blob:none`。仍能看到全部文件，只是读旧版本内容时现场下载。默认关闭。
- **共享对象库**：多个 worktree 共用 mirror 的对象，不重复存历史。

#### 初始化与同步

1. **创建工作区**：先 fetch mirror，再从调用方指定的分支创建 worktree。分支不存在时，从最新主干切出。
2. **子模块和 LFS**：
   - 有 `.gitmodules` 时执行 `git submodule update --init --recursive`。
   - 使用 LFS 时执行 `git lfs pull`，服务端镜像需安装 `git-lfs`。
   - 任一步失败，在工作台顶部提示“上下文不完整”并列出缺失部分，不静默跳过。
3. **开始前同步**：打开工作台时和每轮 AI 开始前：
   - fetch 主仓库和所有从仓库的 mirror；
   - 工作区分支落后主干时，提示“主干有更新”，提供“更新到最新主干”选项（把主干 merge 进文档分支，不 rebase），不自动执行；
   - 从仓库只读，直接检出到各自默认分支的最新提交；
   - 显示当前对照的主干 commit SHA。
4. **被忽略的文件**：`.gitignore` 中的内容（`node_modules`、构建产物、`.env` 等）不在工作区，这是预期行为。

#### 能力

| 能力 | 说明 |
|---|---|
| 列目录 | 懒加载，遵守 `.gitignore`，隐藏 `.git` |
| 读文件 | 返回内容和版本号（内容哈希）；二进制或超大文件只返回元信息 |
| 写文件 | 校验 `baseVersion`，只允许白名单路径，不一致返回 409 |
| 搜索 | 文件名模糊匹配；内容用 `rg`，限制结果数和超时 |
| diff | 工作区相对分支基点的 diff |
| commit / push | 只包含白名单内文件 |
| 文件监听 | `notify` 监听工作区，合并（约 200ms 一批）后推送；AI 修改和外部更新走同一通道 |

文件能力不直接使用 app-server 的 `fs/*`（本机 0.158.0 schema 中有 `fs/readFile`、`fs/readDirectory`、`fs/watch`、`fs/changed`、`fuzzyFileSearch`）。原因：浏览文件不应依赖 Codex 进程，权限控制也应由平台自己掌握。app-server 只负责 AI 部分。

### 4.2 AgentSessionService：AI 会话

#### 进程模型

- 每个工作区最多一个 `codex app-server` 进程，`cwd` 为该工作区。
- 线程 ID 落库；进程空闲超时退出后，用 `thread/resume` 恢复对话。
- 总数受现有 `web-platform/src/concurrency/manager.rs` 限制。
- 把 `rust-platform/src/agent/codex_client.rs` 抽成共享 crate，两个服务共用，不复制。

#### 会话划分（已确定）

- **同一阶段共用一个会话**：例如 spec 阶段内，AI 起草、审阅人追问、AI 修改都在同一会话，AI 记得此前的追问。
- **跨阶段新开会话**：进入下一阶段（如 spec 批准后进入 plan）时新开会话，只读取上一阶段**已批准的文档文件**，不继承上一阶段的对话。
- 会话按 `(workspaceKey, stage)` 落库；退回某阶段时，恢复该阶段的会话。

#### 两种运行模式

| 模式 | 触发 | 用途 |
|---|---|---|
| 交互模式 | 用户在工作台发消息 | 人和 AI 一起写、审阅时追问和修改 |
| 非交互模式 | 流程事件（如 G1、G2 批准）直接触发，无人在线 | 自动起草 spec / plan；完成后由流程方案推进状态 |

两种模式使用同一个工作区和同一个阶段会话。非交互模式的失败重试、卡死检测和配额见 `ai-native-sdlc-flow-design.md` 第 4.7 节。

#### 需要接入的协议能力

以下方法来自本机 0.158.0 的 schema，实施时须用部署版本（当前 0.159.0）执行 `codex app-server generate-json-schema` 重新核对。

| 协议 | 用途 |
|---|---|
| `turn/start`、`turn/steer`、`turn/interrupt` | 发起一轮、运行中追加说明、停止 |
| `item/agentMessage/delta`、`item/reasoning/summaryTextDelta` | 流式消息和思考摘要 |
| `item/started`、`item/completed`、`item/commandExecution/outputDelta` | 读文件、执行命令的过程卡片 |
| `item/fileChange/patchUpdated`、`turn/diff/updated` | 实时显示 AI 改动和本轮汇总 diff |
| `item/fileChange/requestApproval`、`item/commandExecution/requestApproval` | 审批卡片和写入范围控制 |
| `item/tool/requestUserInput` | AI 追问卡片 |
| `turn/completed`、`thread/resume` | 一轮结束、会话恢复 |

#### 读写边界

- **读**：Codex 能读整个工作区，包括主仓库全部代码、历史和规则文件。
- **写**：
  - sandbox 设为工作区可写；
  - 不在 `writablePaths` 内的 fileChange 审批，后端自动拒绝，并告诉 AI 原因；
  - 白名单内的改动**自动批准**，不逐次询问用户（已确定）；用户在本轮结束后通过 diff 逐块接受或回退；
  - 命令执行：只读命令（如 `rg`、`cat`、`ls`、`git log`）自动允许，其他命令按 `agentPolicy` 处理；
  - 每轮结束后用 `git status` 兜底，拒绝范围外的残留改动。
- **待实测**：“workspace-write 不限制读”是对 Codex sandbox 行为的理解，须在部署版本上实测，结果纳入 M3 验收。

#### 回退能力

每轮开始前对白名单内文件做快照。前端可整轮回退或按块回退，后端据此写回，写回同样走版本校验。

#### 平台统一管理的 CODEX_HOME

网页端要和本地 Codex 遵守同样的规则，需要两层配置都生效：

| 层级 | 来源 | 作用 |
|---|---|---|
| 仓库级 | 工作区内的 `AGENTS.md` 和仓库自带 skills | 随仓库自动生效 |
| 组织级 | 平台维护的 `CODEX_HOME` | 团队共享的全局 `AGENTS.md`、skills、`config.toml`（模型、审批默认值、MCP 配置） |

- **来源**：组织级配置放在单独的 Git 仓库（如 `codex-org-config`），变更走 PR，平台定期或按版本同步到服务端。
- **启动**：启动 app-server 时设置 `CODEX_HOME=<平台目录>`，不读运行账号自己的 `~/.codex`。
- **按项目覆盖**：项目可追加自己的 skills 目录，优先级高于组织级。
- **可追溯**：每个会话记录组织配置 commit SHA 和主干 SHA，可查到文档是在哪套规则、哪个代码版本下生成的。
- **凭据**：模型和 MCP 凭据由部署环境注入，不放入配置仓库，也不传给工作区内执行的命令。

#### 安全

- 子进程环境不带 GitHub/GitLab 平台 token。
- 命令执行默认需要审批；只读命令（如 `rg`、`cat`、`ls`）可配置为自动允许。
- 仓库内容一律视为数据，不当作指令。

### 4.3 通信协议

- **REST**：工作区创建、目录、文件、搜索、diff、commit。
- **WebSocket**：每个打开的工作台一条连接。
  - 下行：文件变化、AI 事件、锁状态、上下文完整性状态。
  - 上行：发消息、追加说明、停止、审批应答、回答追问。
  - 鉴权复用现有 SSE ticket 机制（`concurrency::create_sse_ticket`），先换一次性 ticket 再连接。
- 选择 WebSocket 是因为工作台有大量上行操作。axum 需要开启 `ws` feature（实施期依赖改动）。
- 后端转发前做一层归一化，形成稳定的内部事件契约，前端不随 Codex 协议版本变化。

### 4.4 并发与一致性

- **不做编辑权分配**（已确定）：任何打开工作台的项目成员都可以编辑，不区分编辑者和观看者。
- **AI 运行期间的文件锁**：AI 运行一轮时，**本轮正在修改的文件**对人只读，本轮结束后解锁；其他文件不受影响。这条锁只在文档工作台内生效，与实现阶段无关。
- **乐观锁**：所有写入带 `baseVersion`，不一致返回 409，前端进入冲突流程。

## 5. 非功能要求

| 项 | 要求 |
|---|---|
| 性能 | 首屏只加载第一层目录；单文件超过 1MB 只读且不高亮；预览防抖 150ms；AI 增量事件按帧合并渲染 |
| 资源 | app-server 空闲 15 分钟退出（可配置）；工作区接入现有 `rust-platform/src/workspace/gc.rs` 的回收思路 |
| 磁盘 | 监控 mirror 和 worktree 占用；开启 `blob:none` 的项目记录按需下载次数 |
| 上下文完整性 | 每个工作区记录子模块、LFS、主干同步状态，显示在工作台并写入日志 |
| 安全 | 路径规范化，防目录穿越和符号链接逃逸；读权限沿用 `project_access`；审计日志记录 commit 和审批操作 |
| 可观测 | 会话数、每轮耗时、token 用量、审批等待时长、冲突次数、越界写入拒绝次数 |

## 6. 分期与验收

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| M1 只读工作台 | 工作区创建、目录树、多标签、搜索、Markdown 预览 | 网页可浏览任意文件和目录；预览正确渲染表格、代码和 Mermaid；工作区内 `git log` 有完整历史；带子模块或 LFS 的测试仓库检出完整；主干落后时有提示 |
| M2 人工编辑 | 白名单可写、版本冲突、文件监听、commit/push | 两个标签页并发修改能检测冲突；提交后远端可见 commit |
| M3 AI 会话 | app-server 接入、流式对话、过程卡片、实时 diff、审批、追问、停止和追加说明、平台 `CODEX_HOME` | AI 改文档过程实时可见；实测 Codex 能读工作区任意文件、写入限制在白名单；越界写入被拒绝；整轮和按块回退可用；组织规则和 skills 生效；会话记录含组织配置 SHA 和主干 SHA |
| M4 体验打磨 | `@` 引用、选区改写、点击路径跳转、会话恢复 | 刷新页面后对话和未完成状态可恢复 |

## 7. 与完整流程方案的衔接（已全部确定）

以下问题原先留给完整流程方案，现已确定。流程细节见 [ai-native-sdlc-flow-design.md](./ai-native-sdlc-flow-design.md)。

| 问题 | 结论 |
|---|---|
| 1. 调用时机 | Intent、Spec、Plan 三个阶段及其审阅使用工作台；`writablePaths` 为 `.symphony/changes/<iid>/**`；`promptPresets` 来自文档模板的生成 prompt（流程方案第 5 节） |
| 2. 提交策略 | 每次保存即在文档分支 commit / push；一个 Issue 一个文档 MR；流程插槽放“提交审阅、批准、要求修改、拒绝、退回文档阶段”（流程方案第 6、10 节） |
| 3. 分支命名 | `symphony/docs-<iid>`，退回重开时 `symphony/docs-<iid>-r<n>`；一个 Issue 一个工作区 |
| 4. 审批策略 | 白名单内 AI 写入自动批准，不逐次确认（本文 4.2） |
| 5. 会话传递 | 同一阶段共用会话，跨阶段新开会话，只以已批准文档交接（本文 4.2） |
| 6. 编辑者分配 | 不做编辑权分配；只保留“AI 运行期间本轮修改的文件只读”（本文 4.4） |
| 7. 从仓库 | 只读完整检出到 `repos/<alias>/`（本文 4.1） |
| 8. 主干更新 | 文档分支不 rebase；合并前需要同步时把主干 merge 进文档分支（流程方案第 10.2 节第 5 步） |
