# 实现阶段自验证与 AGENTS.md 加载修复方案

> **状态**：设计草案，未实现。
>
> **范围**：
> 1. 实现阶段的测试改为在开发流程内完成（不再使用独立的 `Testing` 状态），并写死默认的前端、后端验证方案。
> 2. E2E 所需的登录 token 由用户在 Issue 中提供，Symphony 不做自动登录。
> 3. 修复模板中失效的 `App runtime validation (required)` 引用。
> 4. 修复 Codex 会话加载了 symphony-rust 自己的 `AGENTS.md`、没有加载目标仓库 `AGENTS.md` 的问题。
>
> **相关文档**：`ai-native-sdlc-flow-design.md`（第 1 节把「Test：自验证、verifier」标为基本具备，本文修正该判断）、`test-engineer-agent.md`（独立 Testing 阶段，本文之后不再作为默认流程）、`teamwork-integration-scheme.md`（多代理实现，本文不依赖它）、`multi-repo-workspace-design.md`。

## 1. 现状核查（2026-10-30 ~ 11-02）

以下结论来自本机部署的代码、数据库和 Codex 会话记录（`~/.codex/sessions`）。

### 1.1 自验证循环

| 环节 | 实现状态 | 是否在工作 | 依据 |
|---|---|---|---|
| 构建、跑测试 | 只有提示词要求 | 是，但 Symphony 不检查结果 | WORKFLOW 要求 push 前跑测试并在 workpad 记录。jst-ai-studio#38 中 AI 跑了 `dotnet build`、`dotnet test`（333 通过）、`vitest`（243 通过）、`tsc`、`lint` |
| 后端 API 测试 | 只有提示词要求 | 部分 | #38 的 API 测试用反射在进程内调用 `Execute()`，没有启动服务、没有发 HTTP 请求，路由、权限和序列化中间件没有测到 |
| 前端集成测试 | 只有提示词要求 | 否 | #38 的新测试 mock 了 `@/services/opsStat` 和 `ChartBox`，只跑了 dashboard 目录，没跑全量 |
| E2E、UI 走查 | 只有提示词要求 | 否 | 仓库有 Playwright（`pnpm test:e2e`），但没有测试账号，AI 跳过了 |
| 截图对比 | 未实现 | 否 | 代码中没有截图基线或对比逻辑，Codex 没有浏览器类 MCP，#38 会话中没有任何截图 |
| 独立 Testing 阶段 | 已实现 | 未启用 | `workflow_test_gitlab.md` 和 `testing_enabled` 存在，6 个项目全部为 0 |
| teamwork reviewer | 只有配置结构 | 否 | `rust-platform` 中只有 `TeamworkConfig` 定义，没有被使用 |
| CI 构建门禁 | 不归 Symphony | 否 | jxyai、jst-ai-studio 都没有 `.gitlab-ci.yml` |

### 1.2 `App runtime validation (required)`

来自上游 Symphony 模板（本仓库首个提交 `258f81b`）。原意是：改动影响应用时，交付前用 `launch-app` skill 把应用跑起来验证，并用 `github-pr-media` skill 把截图或录屏附到 PR。

现状：

- web-platform 的模板删掉了定义这一节的内容，只在完成条件里留了一句「满足 `App runtime validation (required)`」，引用指向不存在的章节。
- `launch-app`、`github-pr-media` 两个 skill 在本项目中都不存在（`.codex/skills` 只有 commit、debug、land、linear、pull、push）。
- 结果：这条要求没有任何效果。

### 1.3 AGENTS.md 加载错误

Codex 从 git 根目录到工作目录（`cwd`）逐级查找并合并 `AGENTS.md`，不会查找 `cwd` 下面的子目录。

本机 `.env.local` 把 `SYMPHONY_WORKSPACE_ROOT` 设为 `symphony-rust/workspaces`，位于 symphony-rust 仓库内部（`.gitignore` 忽略了 `workspaces/`，但 git 根目录仍然是 symphony-rust）。只要 Codex 启动时 `cwd` 本身不是 git 仓库，向上找到的 git 根就是 symphony-rust，加载的是 symphony-rust 自己的 `AGENTS.md`。

已核实的会话：

| 场景 | cwd 示例 | 会话 | 实际加载的 AGENTS.md |
|---|---|---|---|
| 文档阶段（spec/plan 起草） | `workspaces/editor/2/38` | 2026-10-30 14:01 | symphony-rust |
| 多仓库实现 | `workspaces/2/i-3338-_38` | 2026-10-30 15:19 | symphony-rust |
| 多仓库实现 | `workspaces/6/i-32-_2` | 2026-10-26 09:44 | symphony-rust |
| 单仓库实现，旧布局 | `workspaces/1/i-38-_8` | 2026-10-22 17:29 | symphony-rust（会话 17:29:37 启动，仓库文件 17:30:24 才出现：仓库是 AI 在会话里自己 clone 的） |
| 单仓库实现，旧布局 | `workspaces/3/i-31-_1` | 2026-10-27 09:34 | symphony-rust（AI 在会话里执行 `git clone ... repo`，仓库在 `repo/` 子目录） |

影响：spec、plan 和代码都是在没有读到目标项目编码规范的情况下写的，反而带入了 symphony-rust 的规则（例如「web-frontend 用 Node v22」）。这与 `ai-native-sdlc-flow-design.md` 第 8 节「Spec 起草加载目标仓库自带的 AGENTS.md 和 skills」不符。

多仓库还有一个独立的问题：即使工作区不在任何 git 仓库里，`cwd` 是工作区根目录，各子仓库的 `primary/AGENTS.md`、`jxyai/AGENTS.md` 在 `cwd` 下面，Codex 也不会加载。

## 2. 决策

| 编号 | 决策 | 理由 |
|---|---|---|
| D1 | 测试在开发流程内完成，不再使用独立 `Testing` 状态 | 不用打回排队、不重读上下文；状态机简单 |
| D2 | 开发会话写完代码后，**用 Codex subagent 执行验证**，不另开 Symphony 会话 | subagent 有独立上下文，给出一定的独立视角；本机 Codex 0.162.1 的 `multi_agent` 为 stable 且已开启；不改 runner |
| D3 | 模板写死默认验证方案；Issue 或提示词声明了别的测试方案时，以用户声明为准 | 给出可执行的默认值，同时允许按项目覆盖 |
| D4 | E2E token 由用户在 Issue 中提供，Symphony 不做自动登录 | 先解决「有没有」，登录自动化以后再做 |
| D5 | `App runtime validation` 不删除，用默认验证方案中的 E2E 和截图要求重新定义 | 它对应的正是缺失的「实际运行 + 截图证据」 |
| D6 | AGENTS.md 只由各项目在自己的 git 仓库中维护，Symphony **不生成、不复制**任何 AGENTS.md 文件 | 仓库是唯一来源，避免副本过期和误改 |
| D7 | 工作区根目录必须位于任何 git 仓库之外 | 根治「向上找到 symphony-rust」 |
| D8 | 多仓库时，启动会话前读取各仓库当前的 AGENTS.md，放进会话的 `developerInstructions` | 补上 Codex 不查子目录的缺口，且不落盘 |

## 3. 自验证方案

### 3.1 流程

```text
Todo → In Progress ─────────────────────────────────────────► Human Review → Merging → Done
          │                                                        ▲
          ├─ 实现（主会话）                                        │
          ├─ 自验证：主会话调用 subagent 执行「默认验证方案」      │
          │     └─ subagent 返回每项的命令、结果、证据            │
          ├─ 不通过：主会话修复后重新调用 subagent（最多 3 轮）   │
          └─ 通过，或 3 轮仍不通过（写明阻塞项）──────────────────┘
```

- 主会话负责改代码、写 workpad、推送、开 MR；subagent 只负责执行验证并报告，**不改业务代码**，可以新增或修改测试文件，但改动要交回主会话确认后由主会话提交。
- subagent 的提示词由模板提供，包含：本次 diff 范围、plan.md 的 `## Tests`、默认验证方案、Issue 中声明的测试方案、E2E 信息（如有）。
- 3 轮仍不通过时，在 workpad 写明失败项和原因，照常进入 Human Review，由人决定。

### 3.2 默认验证方案

写入 WORKFLOW 模板，标题为 `## App runtime validation (required)`，使现有完成条件的引用有效。

**优先级**：Issue / 提示词中声明的测试方案 > 目标仓库 `AGENTS.md` 中的测试要求 > 本默认方案。用户声明可以增加、替换或跳过某项，跳过必须写明理由。

**后端**（diff 涉及后端代码时）：

| 项 | 要求 |
|---|---|
| 构建 | 项目的标准构建命令，0 错误 |
| 单元测试 | 覆盖新增和修改的业务逻辑 |
| 集成测试 | 连开发环境依赖（数据库等）运行受影响的集成测试 |
| API 测试 | **在本地启动服务，用真实 HTTP 请求调用本次新增或修改的接口**，覆盖正常请求、参数错误、权限不足；记录请求和响应摘要。进程内反射调用不算 API 测试 |

**前端**（diff 涉及前端代码时）：

| 项 | 要求 |
|---|---|
| 静态检查 | 类型检查、lint，0 错误 |
| 单元 / 组件测试 | **全量**运行，不只跑改动目录 |
| 集成测试 | 至少一条不 mock 本次改动的 service 层的测试，或在 E2E 中覆盖（见下） |
| E2E | 有 E2E 信息时必须执行，UI 改动必须新增或更新用例；关键页面截图附到 MR |

**证据**：每一项在 workpad `Validation` 中写明命令和结果摘要。未执行的项不得勾选，必须写明原因（例如「Issue 未提供 E2E token，跳过 E2E」）。

**截图对比**：本期只要求附截图作为证据，不做像素级基线对比。目标仓库已有 Playwright `toHaveScreenshot` 基线时，按仓库约定运行。

### 3.3 E2E token

用户在 Issue 描述或评论中提供，格式约定：

```text
## E2E
- base_url: https://dev.example.com
- token: <登录 token>
- 说明: token 写入 localStorage 的 key 为 token（可选）
```

规则：

- AI 通过环境变量把 token 交给测试（例如 `E2E_BASE_URL`、`E2E_TOKEN`），按目标仓库的 E2E 约定注入登录状态。
- token **不得**写进代码、提交、workpad、MR 描述或日志。workpad 中只写「已使用 Issue 提供的 E2E token」。
- 没有提供时跳过 E2E 并在 workpad 说明，**不得**用伪造的登录状态代替真实 token。

**风险**：Issue 中的 token 是明文，项目成员都能看到，并会原样进入 Codex 会话记录（`~/.codex/sessions`）。只能使用开发环境、最小权限、短期有效的 token，用完作废。这一点写进模板和项目设置页的说明。

### 3.4 Testing 状态的处理

- 模板中「完成后移到 Testing」的分支保留，但只在项目 `testing_enabled = 1` 时渲染（现状即如此）。
- 默认流程不再推荐启用 Testing；项目设置页的开关说明改为「可选的独立复测，默认流程已在开发阶段完成验证」。
- `test-engineer-agent.md` 标注为可选流程。

## 4. AGENTS.md 加载修复

### 4.1 工作区根目录移出 git 仓库

- 部署配置：`SYMPHONY_WORKSPACE_ROOT` 必须指向仓库外的目录。`dev.sh` 默认值本来就是 `~/symphony-workspaces`，问题来自本机 `.env.local`。
- 启动检查：web-platform 启动时对 `SYMPHONY_WORKSPACE_ROOT` 执行 `git -C <root> rev-parse --show-toplevel`（目录不存在时检查其最近的已存在父目录）。命令成功（说明位于某个 git 仓库内）则拒绝启动，错误信息给出当前 git 根和修改建议。
- 文档阶段工作区（`<root>/editor/...`）和实现阶段工作区（`<root>/<project_id>/...`）都在这个根目录下，一处修复两处生效。

### 4.2 单仓库：仓库就绪后再启动 Codex

- 现状：单仓库项目的 WORKFLOW 没有 `after_create` clone hook，Symphony 也不 checkout，工作区是空目录，AI 在会话里自己 clone（`workspaces/1/i-38-_8`、`workspaces/3/i-31-_1`）。会话启动时 cwd 下还没有仓库，AGENTS.md 永远加载不到。
- 修复：单仓库也走 Symphony 的 checkout（复用多仓库的 `prepare_multi_repo_workspace` 的 checkout 实现，只有一个 primary 仓库），**直接 checkout 到工作区根目录**，使 `cwd` 就是仓库根。Codex 启动前确认 `<cwd>/.git` 存在。
- 已有旧布局工作区（`repo/` 子目录、或 AI 自行 clone 的）不迁移，按 `has_legacy_workspace` 的现有规则继续使用，Issue 关闭后由 GC 清理。
- 模板：单仓库模板加一句「Symphony 已在工作区根目录准备好仓库，不要再 clone」。

### 4.3 多仓库：把各仓库 AGENTS.md 放进 developerInstructions

启动 Codex 会话（`thread/start`）时，由 Symphony 读取每个仓库**当前 checkout 的** `AGENTS.md`，拼成一段说明放进 `developerInstructions`：

```text
## Repository instructions
This workspace contains several repositories. Each section below is that repository's own AGENTS.md.
When you read or change files under `<alias>/`, follow that repository's section.
Skills of a repository live in `<alias>/.codex/skills/` when present.

### `primary/` (zimei10525/jst-ai-studio)
<primary/AGENTS.md 原文>

### `jxyai/` (JstAi/jxy)
<jxyai/AGENTS.md 原文>
```

- 只读取、不写文件（D6）。仓库没有 AGENTS.md 时省略该段。
- 只读取仓库根目录的 `AGENTS.md`（以及 Codex 支持的 `AGENTS.override.md`，存在时优先）。子目录中的 AGENTS.md 由 AI 进入子目录工作时按需读取，模板中提示这一点。
- 长度上限：与 Codex 默认 `project_doc_max_bytes`（32 KiB）对齐，所有仓库合计不超过 32 KiB。超过时按仓库顺序保留原文，放不下的仓库只写路径，并在说明中要求「动手前先读取 `<alias>/AGENTS.md`」。
- 读取的是 checkout 之后、会话启动之前的版本。Rework 换分支或 pull 之后不重新注入；模板要求 AI 在 pull 后若发现 AGENTS.md 有变更则重新阅读。
- 适用位置：
  - 实现阶段：`rust-platform` 的 `build_thread_start_request`（目前不传 `developerInstructions`，需要新增参数）。
  - 文档阶段：`web-platform/src/sdlc/agent.rs` 已经传 `developerInstructions`，在 `service.rs` 的 `developer_instructions()` 后追加。
- 单仓库不注入（Codex 自己会加载），避免重复。

### 4.4 验证

- 单元测试：拼接逻辑（缺文件、override 优先、超长截断、顺序稳定）。
- 集成测试：启动检查在仓库内路径拒绝启动、仓库外路径正常。
- 端到端：在新根目录下各跑一次文档会话、单仓库实现会话、多仓库实现会话，检查会话记录：
  - 单仓库：`AGENTS.md instructions for <cwd>` 的内容是目标仓库的 AGENTS.md。
  - 多仓库和文档阶段：`developerInstructions` 中包含各仓库的 AGENTS.md，且不再出现 symphony-rust 的 AGENTS.md 内容（例如 `Frontend Node Environment` 一节）。

### 4.5 本机迁移（运维操作，需确认后执行）

1. 停止 web-platform 和所有项目 worker，确认没有运行中的 AI 会话。
2. 备份 `data.db` 和 `workspaces/`。
3. 把 `workspaces/` 移到 `~/symphony-workspaces`，修改 `.env.local` 的 `SYMPHONY_WORKSPACE_ROOT`。
4. 在 `editor/mirrors/*` 等 bare 镜像中执行 `git worktree repair <新路径>...`（worktree 记录的是绝对路径）。
5. 更新数据库和各项目 `WORKFLOW.md` 中记录的工作区路径（`workspace.root` 等）。
6. 启动服务，按 4.4 验证。进行中的 Issue（例如 jst-ai-studio#37）重点检查。

## 5. 任务拆分

| PR | 内容 | 涉及文件 |
|---|---|---|
| PR-1 | 工作区根目录启动检查；多仓库和文档阶段注入 AGENTS.md | `web-platform/src/main.rs`、`web-platform/src/sdlc/service.rs`、`rust-platform/src/agent/codex_client.rs`、`rust-platform/src/agent/runner.rs`、新增共用的 AGENTS.md 读取函数 |
| PR-2 | 单仓库由 Symphony checkout 到工作区根目录 | `rust-platform/src/workspace/mod.rs`、`rust-platform/src/agent/runner.rs`、`web-platform/src/templates/mod.rs`（单仓库也渲染 `workspace.repos`）、模板 |
| PR-3 | 默认验证方案、subagent 自验证、E2E token 规则、`App runtime validation` 章节 | `web-platform/src/templates/workflow_{gitlab,github,gitea}.md` 及 snapshots、项目设置页 Testing 开关说明 |
| 运维 | 本机迁移（4.5） | 无代码改动 |

PR-1 和 PR-3 互不依赖，可以并行。PR-2 改动 checkout 时序，风险最高，单独审查。

## 6. 未解决问题

- subagent 的权限：Codex subagent 继承主会话的沙箱和审批策略，目前无法限制它只改测试文件，只能靠提示词约束。
- 多仓库 `developerInstructions` 与 Codex 自身加载的 AGENTS.md 同时存在时的优先级以 Codex 实现为准，需要在 4.4 中实测。
- 目标仓库的 skills（`<alias>/.codex/skills`）在多仓库下不会被 Codex 自动发现，本期只在说明中告诉 AI 位置，是否需要通过 `CODEX_HOME` 合并 skills 另行评估。
