# WORKFLOW.md 平台适配改写方案（v2 — 对抗验证修订版）

## 背景

当前 `WORKFLOW.md` 的提示词模板基于 Linear 工作流编写，包含大量 Linear 特有的概念（Linear MCP、`update_issue` API、原生 state 字段）。项目已支持 GitLab 和 GitHub 两种 tracker kind，需要产出对应版本的 workflow 模板。

## 产出物

| 文件 | 说明 |
|------|------|
| `WORKFLOW.md.gitlab` | GitLab 版本，使用 `glab` CLI + GitLab REST API |
| `WORKFLOW.md.github` | GitHub 版本，使用 `gh` CLI + GitHub REST API |

## 设计决策（对抗验证后确认）

### D1: 状态管理边界

- **Orchestrator（只读）**：通过 Platform trait 轮询 issue labels，推导 workflow_state，做调度和终止决策
- **Agent（读写）**：通过 CLI 工具执行状态变更（修改 labels）、评论、MR/PR 操作
- **无竞态**：Orchestrator 不写 labels，Agent 不与 Orchestrator 通信状态变更。Agent 完成工作后将 issue 移到非 active 状态（如 Human Review），下一个 poll tick Orchestrator 发现 issue 不再 active，正常终止 worker。

### D2: Label 命名约定

直接使用 `active_states` 配置中的值作为 GitLab/GitHub 上的 label 名称。例如配置 `active_states: [Todo, "In Progress"]`，则 GitLab/GitHub 上需要预创建名为 `Todo`、`In Progress` 的 labels。

**不使用 `workflow::` 前缀**，原因：
- 现有代码 `ServiceConfig` 将 `active_states` 值直接作为 API 查询 filter 和 label 匹配值
- 无需修改 Rust 代码即可工作
- 用户可自由选择 label 命名风格（只要配置与实际 label 一致即可）

### D3: 模板选择机制

用户手动选择对应 kind 的 workflow 文件，通过 CLI 参数或 `SYMPHONY_WORKFLOW` 环境变量指定路径。代码无需自动选择逻辑。

### D4: Worker 生命周期与状态流转

Agent 将 issue 移到 `Human Review` 后，该状态不在 `active_states` 中，Orchestrator 下一个 tick 会正常终止 worker。这是预期行为——Agent 完成实现工作后应主动退出。

## 分析：平台核心差异

### 1. 状态管理机制

| 维度 | Linear | GitLab | GitHub |
|------|--------|--------|--------|
| 状态存储 | 原生 `state` 字段 | Label | Label |
| 状态变更 | `update_issue(state: "In Progress")` | `glab issue update <iid> --label "In Progress" --unlabel "Todo"` | `gh issue edit <number> --add-label "In Progress" --remove-label "Todo"` |
| 状态读取 | GraphQL query | Issue labels 过滤 | Issue labels 过滤 |
| 原子性 | 原子操作 | 单次 PUT 支持同时 add+remove | 单次 `gh issue edit` 支持同时 --add-label + --remove-label |

### 2. Issue 交互工具（修正后）

| 操作 | GitLab (`glab`) | GitHub (`gh`) |
|------|----------------|---------------|
| 读取 issue | `glab issue view <iid> --json` | `gh issue view <number> --json labels,title,body` |
| 修改状态 | `glab issue update <iid> --label "X" --unlabel "Y"` | `gh issue edit <number> --add-label "X" --remove-label "Y"` |
| 创建评论 | `glab issue note <iid> -m "..."` | `gh issue comment <number> -b "..."` |
| 更新评论 | `glab api PUT "projects/:id/issues/:iid/notes/:note_id" -f body="..."` | `gh api PATCH "repos/:owner/:repo/issues/comments/:id" -f body="..."` |
| 查找评论 | `glab api "projects/:id/issues/:iid/notes?per_page=100" \| jq '[.[] \| select(.system==false)]'` | `gh api "repos/:owner/:repo/issues/:number/comments" --paginate` |
| 创建 issue | `glab issue create --title "..." --label "Backlog"` | `gh issue create --title "..." --label "Backlog"` |
| 创建 MR/PR | `glab mr create --title "..." --description "Closes #<iid>"` | `gh pr create --title "..." --body "Closes #<number>"` |
| 查看 MR/PR 反馈 | `glab api "projects/:id/merge_requests/:mr_iid/notes?per_page=100"` | `gh pr view <number> --comments` + `gh api "repos/:owner/:repo/pulls/:number/comments"` |
| 合并 MR/PR | `glab mr merge <mr_iid> --squash` | 通过 land skill（不直接调用 `gh pr merge`） |

### 3. 标识符格式

| 平台 | Issue 标识 | PR/MR 标识 | `{{ issue.identifier }}` 渲染值 |
|------|-----------|-----------|-------------------------------|
| GitLab | `#42` (iid) | `!15` (MR iid) | `#42` |
| GitHub | `#42` (number) | `#15` (PR number) | `#42` |

### 4. Project ID 获取（GitLab 特有）

GitLab API 路径需要数字 project_id。Agent 获取方式：
```bash
# 方式1：从 glab 获取
PROJECT_ID=$(glab api "projects/$(git remote get-url origin | sed 's|.*gitlab.com/||;s|\.git$||' | jq -Rr @uri)" | jq '.id')

# 方式2：使用 URL-encoded path
glab api "projects/group%2Fproject/issues/42/notes"

# 方式3：从环境变量（推荐，在 hooks 中设置）
glab api "projects/$CI_PROJECT_ID/issues/$ISSUE_IID/notes"
```

## 改写策略

### 保留不变的部分（通用逻辑）

1. **YAML front matter 结构** — `tracker.kind` 字段区分版本，其余配置项通用
2. **Liquid 模板变量** — `{{ issue.identifier }}`, `{{ issue.title }}` 等由 PromptEngine 统一注入
3. **整体工作流结构** — Step 0-4 的流程骨架
4. **Continuation context 块** — attempt/turn 逻辑与平台无关
5. **Git 操作指令** — commit、push、rebase 等
6. **Workpad 模板结构** — `## Codex Workpad` 格式
7. **Guardrails** — 通用约束规则
8. **PR feedback sweep protocol** — 逻辑相同，仅命令不同
9. **Completion bar** — 质量门槛通用

### 需要平台化改写的部分

| 章节 | 改写内容 | GitLab 版本 | GitHub 版本 |
|------|---------|------------|------------|
| 开头声明 | "Linear ticket" → 平台 issue | "GitLab issue" | "GitHub issue" |
| Prerequisite | Linear MCP → 平台 CLI | `glab` CLI + `GITLAB_TOKEN` | `gh` CLI + `GITHUB_TOKEN` |
| Related skills | `linear` skill → 平台交互 | `glab`: interact with GitLab | `gh`: interact with GitHub |
| Status map | 状态变更方式 | `glab issue update --label/--unlabel` | `gh issue edit --add-label/--remove-label` |
| Step 0 | 状态读取和路由 | `glab issue view --json` | `gh issue view --json` |
| Step 1 | 评论 CRUD | `glab issue note` / `glab api` | `gh issue comment` / `gh api` |
| Step 2 | MR/PR 操作 | `glab mr create/view` | `gh pr create/view` |
| Step 3 | Merge 操作 | `glab mr merge` | land skill |
| Out-of-scope | "file a Linear issue" | `glab issue create` | `gh issue create` |

### 环境前置条件

Agent 执行环境必须满足：

1. **CLI 工具已安装**：`glab`（GitLab）或 `gh`（GitHub）在 PATH 中可用
2. **认证已配置**：
   - GitLab: `GITLAB_TOKEN` 环境变量（scope: `api`）
   - GitHub: `GITHUB_TOKEN` 环境变量（scope: `repo`）
3. **网络可达**：Codex sandbox 允许访问 GitLab/GitHub API
4. **shell_environment_policy**: 配置为 `inherit=all`（继承 token 环境变量）

建议在 `hooks.after_create` 中验证：
```bash
# GitLab
command -v glab >/dev/null || { echo "glab CLI not found"; exit 1; }
glab auth status || { echo "glab not authenticated"; exit 1; }

# GitHub
command -v gh >/dev/null || { echo "gh CLI not found"; exit 1; }
gh auth status || { echo "gh not authenticated"; exit 1; }
```

### Rate Limit 注意事项

Agent 通过 CLI 操作绕过了 Symphony HttpClient 的 rate-limit 保护。在 workflow 模板中添加指导：
- 避免在循环中频繁调用 API（如轮询 MR 状态）
- 优先使用批量操作（单次 label 变更而非多次）
- GitLab: 默认 300 req/min（认证），GitHub: 5000 req/hour（认证）

### Workpad 评论操作注意事项

1. **GitLab system notes 过滤**：GitLab 的 notes API 返回系统自动生成的 notes（如 label 变更记录），查找 workpad 时必须过滤 `system: true`
2. **分页**：评论多时需要分页查询（GitLab `per_page=100`，GitHub `--paginate`）
3. **幂等性**：创建 workpad 前先查找，避免 continuation 时重复创建

## 命名规范

- `WORKFLOW.md.gitlab` — GitLab 版本
- `WORKFLOW.md.github` — GitHub 版本
- 原 `WORKFLOW.md` 保留作为 Linear 版本参考

## 对抗验证修复记录

| # | 问题 | 严重程度 | 修复 |
|---|------|---------|------|
| 1 | `glab` flag 错误（`--label-add`→`--label`，`--label-rm`→`--unlabel`） | Critical | 已修正所有命令 |
| 2 | Agent/Orchestrator 竞态 | Critical | 明确边界：Orchestrator 只读，Agent 只写（D1） |
| 3 | Reconciler 终止时序 | Critical | 确认为预期行为，已文档化（D4） |
| 4 | `glab issue view --output json` 不存在 | High | 改为 `--json` |
| 5 | `glab mr view --comments` 不存在 | High | 改为 `glab api .../notes` |
| 6 | Rate limit 缺失 | High | 添加 Rate Limit 章节 |
| 7 | 模板选择机制未说明 | High | 明确为用户手动选择（D3） |
| 8 | `active_states` 语义歧义 | High | 明确为实际 label 名（D2） |
| 9 | system notes 过滤 | Medium | 添加到 Workpad 注意事项 |
| 10 | project_id 获取 | Medium | 添加专门章节 |
| 11 | Label 大小写 | Medium | 明确：配置值必须与平台 label 完全一致 |
| 12 | 分页问题 | Medium | 添加到 Workpad 注意事项 |
| 13 | `{{ issue.state }}` 为空 | Medium | 在模板中添加条件处理 |
| 14 | CLI 可用性 | Medium | 添加环境前置条件章节 |
| 15 | GitHub 原子性描述错误 | Low | 修正：`gh issue edit` 支持单次调用同时操作 |
