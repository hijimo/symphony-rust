# AI 原生 SDLC 完整流程方案（文档闸门）

> **范围**：本方案定义 intent / spec / plan 三份文档的流程、闸门、文档状态、文档模板，以及与现有实现流程的衔接。
> 网页编辑能力复用公共编辑器组件，见 [ai-doc-workbench-design.md](./ai-doc-workbench-design.md)，本文不重复。
>
> **平台范围**：第一期支持 GitLab、GitHub（2026-10-27 修订：Gitea 移出第一期，web-platform 尚无 Gitea 平台客户端）。Gitea 与 Linear 项目一律按 `track:fast` 处理。
>
> **状态**：设计草案，未实现。第 11 节为决策记录，全部决策已确定。
>
> **审阅**：对抗审阅见 `docs/quality/ai-native-sdlc-design-review-2026-10-26.md`，本文已按其结论修订。
>
> **依据**：
> - The AI-native SDLC playbook（Anthropic，https://claude.com/resources/articles/the-ai-native-sdlc-playbook ），知识库页 `tianbook-llm-wiki/wiki/content/ai-native-sdlc-playbook.md`。
> - 总结文档 `bilibili-docs/BV1tJem6eEg6/ai-native-sdlc-animation.html` 第四节“一项工作的完整流转”。
> - 当前项目：`web-platform/src/templates/workflow_*.md`、`docs/business.md`、`docs/方案/test-engineer-agent.md`、`docs/方案/teamwork-integration-scheme.md`。

## 1. 现状与差距

Symphony 目前覆盖 Playbook 的 **Build 后半段到 Merge**：Issue 进入 Todo 后，agent 实现、自测、开 MR，人审核并合并。

| Playbook 阶段 | 现状 | 差距 |
|---|---|---|
| Plan：intent.md | “AI 生成 Issue”，结果写进 Issue 正文 | 没有 intent 产物和接受闸门 |
| Design：spec.md | 无 | 整个阶段缺失 |
| Build：plan.md（plan mode） | 计划写在 workpad 评论里，agent 自写自执行 | 没有“计划被接受前不实现”的闸门；计划不入库，无法与 diff 对照 |
| Build：实现 | Codex / teamwork 在隔离工作区实现 | 基本具备 |
| Test：自验证、verifier | Validation、Testing 阶段、teamwork reviewer | 基本具备 |
| Deploy：AI 审查 PR | 部分具备 | 不对照 spec / plan；没有 REVIEW.md |
| Deploy：code owner 批准 | Human Review → Merging | 具备 |
| Deploy：生产发布 | 不在 Symphony 范围 | 由目标项目 CI/CD 负责 |
| Maintain：控制带 → 新 intent | 告警只监控 Symphony 自身（`web-platform/src/alert/rules.rs`） | 没有外部信号 → intent 的入口 |

本方案的主要工作：**在 Todo 前补上三道文档闸门**，并让后续流程读取这些文档。Maintain 闭环放到后期。

## 2. 目标流程

```text
              ┌──────────── 文档阶段（新增）────────────┐   ┌──── 实现阶段（现有，小改）────┐
入口 ──► Intent ──► Intent Review ──► Spec ──► Spec Review ──► Plan ──► Plan Review ──► Todo ──► In Progress ──► Testing ──► Human Review ──► Merging ──► Done
          人+AI       G1 产品负责人     AI 起草   G2 产品负责人   AI 起草   G3 工程师        （读取 plan.md）           对照 spec/plan     code owner
          工作台                                  +政策负责人                                                       的审查
             ▲              │打回                 │打回                  │打回
             │              ▼                     ▼                      ▼
             │          回到对应起草状态，审阅意见作为下一轮输入
             │
外部信号（告警、Sentry、扫描、值班）──► AI 写 intent ──► Intent Review（P3）
```

| Issue 状态 | 执行者 | 做什么 | 产物 |
|---|---|---|---|
| `Intent` | 提出者 + AI（工作台交互） | 用提出者的话写：要什么、为什么、约束、待解决问题 | `intent.md` 草稿 |
| `Intent Review`（G1） | 产品负责人 | 审阅、修正，接受或拒绝 | 接受：定稿；拒绝：关闭 Issue |
| `Spec` | AgentSessionService 自动起草（非交互，只能写文档目录，见 4.7） | 读 intent 和代码，加载组织 skills，写 spec 并标出关注点 | `spec.md` 草稿 |
| `Spec Review`（G2） | 产品负责人 + 被点名的政策负责人 | 在工作台审阅，可让 AI 修改；关注点逐条处理后签字 | `spec.md` 定稿 |
| `Plan` | AgentSessionService 自动起草（只读代码，只写文档） | 写要改的文件、顺序、证明测试、风险、放弃的方案 | `plan.md` 草稿 |
| `Plan Review`（G3） | 工程师 | 在工作台追问 AI，迭代到没看过对话的同事也能照着实现 | `plan.md` 定稿 |
| `Todo` 之后 | 现有流程 | 以 plan.md 为准实现，见第 7 节 | diff、MR |

规则：

- **AI 起草，人审阅**。Spec 和 Plan 在进入状态时由 web-platform 的 AgentSessionService 自动生成草稿，人第一次介入就是审阅。
- **打回走原路**。审阅人“要求修改”并写意见，Issue 回到对应起草状态。AI 在**当前文档**上按意见修改，不从模板重写，审阅人在工作台里做过的修改会保留；修改结果以 diff 呈现，可逐块接受或回退。
- **快速通道**。按路径 label 跳过部分闸门，见第 11 节决策 3（默认 `track:plan`）。
- **入口（已确定）**：文档流程**只从 Symphony 界面进入**：
  - “新建需求”：创建 Issue 并开始 intent，路径为 `track:full`；
  - Issue 卡片上的“开始文档流程”：选择路径，默认选中 `track:plan`。
  - 在 GitLab / GitHub 上直接创建、没有路径 label 的 Issue，停在“计划”列的“待启动”，不自动起草。
  - 原因：web-platform 不轮询 Issue，编排器不处理 `doc:*`；从界面进入可以避免两个服务争抢同一个 Issue，也不会给每个随手创建的 Issue 消耗 token。

## 3. 文档存放

- 路径（已确定，不可配置）：`docs/changes/<iid>/intent.md`、`spec.md`、`plan.md`，放在主仓库（多仓库项目也放主仓库）。`track:plan` 的 Issue 只有 `plan.md`。
- 不用 `.symphony/`：现有代码和 teamwork 方案把 `.symphony/` 当作不提交的运行时目录（锁、tmp、quarantine、handback，且写入 `.git/info/exclude`），文档放在里面会提交不上去。
- 项目级文档模板放在 `docs/changes/_templates/{intent,spec,plan}.md`（第 5.2 节）。`_templates` 不是数字，不会与 Issue 号冲突。
- 不放在该目录的内容：审批状态和会话记录（数据库）、运行时临时文件（仍在 `.symphony/`，不提交）。
- 创建文档前检查 `docs/changes/<iid>/` 是否已存在且不属于本 Issue，存在时报错，不覆盖。
- 文档分支：每个 Issue 一个 `symphony/docs-<iid>` 分支和一个文档 MR。工作台和起草 agent 都在该分支上工作。
- 合入主干的时机：一个文档 MR，G3 通过后合入（第 11 节决策 1，流程见第 10 节）。

## 4. 文档状态

### 4.1 两层状态，各有唯一来源

| 层级 | 管什么 | 唯一来源 | 谁改 |
|---|---|---|---|
| 文档阶段的流程状态（`doc:*`） | 工作处在哪道文档闸门 | Symphony 数据库（由文档状态推导），label 只做显示 | 只有 web-platform |
| 实现阶段的流程状态（Todo 之后） | 实现进度，编排器靠它调度 | Issue label（沿用现有机制） | 编排器、agent（现有约定） |
| 文档状态 | 每份文档的生命周期与批准记录 | Symphony 数据库 | 工作台、闸门操作 |

两个阶段的交接点是“文档 MR 合并成功”：web-platform 把 `doc:plan-review` 换成项目 WORKFLOW 配置中 Todo 状态对应的 label（不写死字符串），之后由编排器接管。

**人工修改 label（已确定）**：web-platform 不监听、也不自动改回 label。人手动修改文档阶段的 label 视为人工覆盖：

- web-platform 下次读取（打开看板或工作台）发现 label 与文档状态不一致时，在卡片上提示“label 与文档状态不一致”，由人处理。
- 人把文档阶段的 Issue 直接改成 `Todo`，视为跳过文档阶段，按 `track:fast` 处理；编排器照常领取。这条绕过路径第一期不拦截。

**已确定**：审批状态只放数据库，文件里不写。原因是批准动作如果改文件，SHA 随之变化，“批准绑定 SHA”无法成立。

### 4.2 单份文档状态机

```text
draft ──提交审阅──► in_review ──批准──► approved ──本文档被修改──► in_review
  ▲                   │                    │
  │               要求修改                  │ 上游文档被重新批准
  └── changes_requested ◄┘                  ▼
                                          stale ──重新审阅──► in_review
approved ──文档 MR 合并──► merged（见下文 merged 阶段）
in_review ──拒绝──► rejected（终态；只有 intent 可被拒绝，拒绝即关闭 Issue）
approved ──被新版本替代──► superseded（保留历史）
```

- **批准**记录：批准人、时间、闸门、文件 blob SHA、commit SHA。
- **批准后被修改**（仅限文档 MR 合并前）：文件 SHA 与批准时不一致，文档自动退回 `in_review`，批准作废。
- **SHA 以 blob 为准**：批准绑定文件 blob SHA，`commit_sha` 只作参考。文件内容不变，blob SHA 就不变，不受分支合并方式影响。

#### merged 阶段（已确定）

文档 MR 合入主干后，三份文档进入 `merged` 阶段：

- 不再做 SHA 比对，也不触发 4.3 的级联。
- 实现阶段修改 `plan.md`（偏离计划时同一 commit 更新）只走代码 MR，由 Human Review 审阅，记为“实现期修订”。
- 要改需求或计划，必须由人主动退回，走第 10.2 节第 17 步的 `symphony/docs-<iid>-r2` 流程。系统不会因为文件变化自动把 Issue 拉回文档阶段，避免终止正在运行的实现 agent。

### 4.3 上游变更自动级联（仅限 merged 之前）

**已确定**：自动退回，只在文档 MR 合并前生效。依赖关系 intent → spec → plan。上游文档被重新批准时：

- 下游已批准文档变为 `stale`；
- Issue 流程退回到最上游的 stale 文档对应的 Review 状态；
- 计一次“需求返工”，用于度量（第 8 节）。

### 4.4 文件内元信息

frontmatter 由系统维护，工作台中只读：

```markdown
---
symphony:
  doc: spec
  issue: 123
  template: spec@3
  generated_by: { org_config_sha: a1b2c3, base_sha: d4e5f6 }
---
```

frontmatter 只在文档创建时写入一次，之后系统不再修改。批准绑定和变更检测使用的“blob SHA”，按去掉 frontmatter 后的正文计算（严格说是正文哈希，不是 git 原生 blob SHA；git blob SHA 另存作参考），所以即使有人在本地误改 frontmatter，也不会让批准失效。

本地 VS Code / Kiro 打开时可看到所属 Issue、模板版本和生成时的规则版本。审批状态在 Symphony 界面、MR 评论和 Issue 评论中查看。

### 4.5 Issue 状态由文档状态推导

```text
intent ∈ {draft, changes_requested}   → Intent
intent = in_review                    → Intent Review
intent = approved 且 spec 不存在      → Spec（触发自动起草）
spec ∈ {draft, changes_requested}     → Spec
spec = in_review                      → Spec Review
spec = approved 且 plan 不存在        → Plan（触发自动起草）
plan ∈ {draft, changes_requested}     → Plan
plan = in_review                      → Plan Review
plan = approved 且文档 MR 未合并      → Plan Review（子状态“已批准，待合并”）
三份文档均为 merged                    → Todo
```

文档阶段的 label 只由 web-platform 根据文档状态写入；编排器不调度 `doc:*`，只在 label 变为 Todo 后接管（4.1、4.7）。

### 4.6 数据表（概要）

| 表 | 主要字段 |
|---|---|
| `change_documents` | project_id、issue_iid、doc_type、status、path、current_blob_sha、template_ref、org_config_sha、base_sha、updated_by、updated_at |
| `document_approvals` | document_id、gate、action（approve / request_changes / reject）、actor、comment、blob_sha、commit_sha、created_at、invalidated_at |

`change_documents.status` 增加 `merged` 取值；另记 `docs_mr_iid`、`merge_commit_sha`、`revision`（r1、r2…）。

### 4.7 文档起草由谁运行（已确定）

**由 web-platform 的 AgentSessionService 运行**，不由 rust-platform 编排器运行。

| 维度 | 编排器（未采用） | AgentSessionService（采用） |
|---|---|---|
| 工作区 | 编排器的 Issue 工作区，与工作台不是同一个目录 | 与工作台共用同一个文档 worktree，AI 写完人原地审阅 |
| 触发 | 轮询 label，最多延迟一个轮询周期 | 批准事件直接触发 |
| 提示词 | 所有 active 状态共用一份 WORKFLOW 提示词 | 使用文档模板的生成 prompt，与模板版本绑定 |
| label 写入方 | agent 自己切换，与“文档阶段以数据库为准”冲突 | 只有 web-platform 写 |
| 依赖项目服务 | 项目服务停止时文档起草也停 | web-platform 常驻，不依赖项目服务 |
| 平台 Token | 子进程带 `GITLAB_TOKEN` / `GITHUB_TOKEN` | 子进程不带平台 Token，commit / push 由服务端完成 |
| 重试与卡死检测 | 现成 | 需要新做，复用 rust-platform 的退避策略和 `codex_client.rs` |

约定：

- 编排器的 `active_states` 不包含 `doc:*`，编排器只负责 Todo 之后。
- 起草会话非交互运行，失败按指数退避重试，连续失败 3 次在卡片上标“起草失败”，提供“重试”和“改为人工撰写”。
- 卡死检测沿用 `stall_timeout_ms` 思路：会话长时间无事件即终止并计一次失败。
- 起草会话计入 web-platform 并发管理器，单独设文档会话配额，并统计 token 用量。
- 非交互起草期间，需要审批的命令一律自动拒绝，只读命令自动允许；同一会话同一时间只运行一轮（工作台方案 4.2）。
- 认证沿用编排器的 `CODEX_HOME`，不需要重新登录（工作台方案 4.2）。

## 5. 文档模板

**已确定**：第一期不做变体，只做 intent、spec、plan 三类各一套模板。

### 5.1 模板组成

| 部分 | 作用 | 举例 |
|---|---|---|
| 骨架 | 新建文档时的初始内容 | intent：Problem / Proposed outcome / Affected users and systems / Constraints / Open questions（Playbook 原文模板） |
| 写作指引 | 每节写什么，以注释放在骨架中 | `<!-- 用提出者自己的话，不要正式语言 -->` |
| 生成 prompt | AI 起草或改写时使用 | spec 使用 Playbook 原文 prompt：“Read the attached intent.md … Describe clearly any areas of concern …” |
| 提交校验 | 提交审阅前的确定性检查 | 必填章节非空；spec 的 Concerns 每条有负责人和结论；plan 有固定格式的 `## Files` 小节和测试。`## Files` 每行一个路径，路径与实现阶段工作区的相对路径完全相同：单仓库项目不加前缀，多仓库项目以仓库别名开头（如 `frontend/...`、`backend/...`）。出现实现阶段不可写仓库（项目配置 `writable: false`）的文件时，提示“该仓库在实现阶段不可写” |
| 审阅清单 | 闸门审批人看到的检查项 | G2：intent 的 Open questions 是已解决，还是被带到下一步？G3：可能弄坏什么？风险最大的一步？放弃了哪些方案？plan 中是否有与需求无关的操作（外部网络访问、修改 CI、读取凭据等）？ |

### 5.2 存放与优先级

```text
内置默认   web-platform 内置，基于 Playbook 原文模板
   ↓ 覆盖
组织级     codex-org-config 仓库 templates/，变更走 PR（第一期不实现）
   ↓ 覆盖
项目级     目标仓库 docs/changes/_templates/{intent,spec,plan}.md
```

- 模板放 Git，不放数据库。组织级模板同时打包成 skill（如 `symphony-docs`），本地 Codex / Kiro 写文档时用同一份模板。
- **第一期**只有两层：内置默认 → 项目级。组织级见 [codex-org-config-requirements.md](./codex-org-config-requirements.md)。
- 与 `web-platform/src/templates/`（WORKFLOW 提示词模板）是两回事，只借用“内置 + 覆盖”的思路。

### 5.3 版本

- 模板带版本号（如 `spec@3`），文档创建时写入 frontmatter 的 `template`。
- 模板升级不影响已有文档。

### 5.4 界面

- **项目设置 → 文档模板**：列出生效模板并标注来源（内置 / 组织 / 项目），支持预览和试生成；编辑项目级模板复用公共编辑器组件，保存即提交到仓库。
- **工作台**：新建文档按模板填骨架；提交审阅前跑校验，未通过项定位到对应章节；审阅人在侧栏看到审阅清单。

## 6. 闸门与审批人

**第一版（已确定）**：当前用户体系没有角色信息，**任何项目成员都可以批准任意闸门**。

| 闸门 | Playbook 对应角色（供以后参考） | 第一版谁能批准 |
|---|---|---|
| G1 Intent | 产品负责人 | 任何项目成员 |
| G2 Spec | 产品负责人 + 政策负责人 | 任何项目成员 |
| G3 Plan | 工程师，高风险加技术负责人 | 任何项目成员 |
| Human Review | Code owner | 沿用平台 MR 审批；写代码的 agent 不能批准自己的 MR |

- 批准、要求修改、拒绝通过工作台流程插槽按钮完成。
- 即使谁都能批，每次审批仍记录**具名批准人**、时间和 SHA（`document_approvals`），审计链里的“谁批准的”不缺失。
- Symphony 记录审批事件后，在 Issue 和文档 MR 上发带 SHA 的评论，再切换 label。
- 以后引入角色时，再增加每道闸门的审批人配置（第 12 节分期）。“发起人不能批准自己的文档”第一期、第二期都不启用。数据模型不需要改。

## 7. 对现有实现阶段的改动

1. **提示词模板**（`workflow_*.md` 和 teamwork 的任务提示词模板）：新增“已批准文档”段，注入三份文档路径和 SHA，要求动手前读 `plan.md`。同时说明：plan.md 只描述改动范围和步骤，不能覆盖 WORKFLOW 中的安全规则和禁止事项。G3 是防止仓库内容中的提示注入经 plan.md 传到实现阶段的人工防线。
2. **workpad 去重**：workpad 只记录执行进度，计划以 `plan.md` 为准。
3. **偏离计划**：偏离时在同一个 commit 更新 `plan.md`。发布前增加确定性检查：比较 diff 文件与 plan.md 的 `## Files` 小节，不一致时在 MR 标注。多仓库项目按仓库目录前缀分仓库比较，前缀与实现阶段多仓库工作区的目录布局一致。第一期只标注不阻断。
4. **审查对照文档**：Testing / teamwork reviewer 增加一个 pass，对照 spec.md 和 plan.md 检查合规；目标仓库可提供 `REVIEW.md`。
5. **Rework 分级**：Human Review 打回时，问题在需求或计划层面的，可直接退回 `Spec Review` 或 `Plan Review`。

## 8. 规则、知识与度量

**规则与知识**（承接工作台方案的平台 `CODEX_HOME`）：

- 品牌、安全、合规、UX 政策写成组织 skills，放在 `codex-org-config`，Spec agent 必须加载。**第一期不实现**（需求见 [codex-org-config-requirements.md](./codex-org-config-requirements.md)）：Spec 起草只加载目标仓库自带的 `AGENTS.md` 和 skills，组织政策要到 Human Review 才可能被发现。
- 每份 spec 记录生成 prompt、组织配置 SHA（第一期为空）、主干 SHA。
- P3：`codex-org-config` 的 CI 用 20–50 个真实任务跑 eval。

**度量**（来自闸门事件和 Git 时间戳）：

| 指标 | 计算方式 |
|---|---|
| intent 存活率 | G1 接受数 ÷（接受数 + 拒绝数） |
| 设计阶段周期 | G1 批准到 G2 批准（spec 由 AI 自动起草，这段时间主要反映人的审阅周期） |
| 需求返工 | plan 首次提交后 spec 又被修改的次数（含 4.3 的级联退回） |
| 首轮合并率 | 未经 Rework 即合并的比例 |
| 计划一致性 | 合并时 diff 与 plan.md 文件清单一致的比例 |

## 9. 看板分列

**已确定**：项目看板和全局看板按 Playbook 的六个阶段重新分列。**分列只是展示层的映射，不新增、不修改任何 label**。label 仍按第 4.5 节由 Symphony 根据文档状态写入，编排器也只看 label。

### 9.1 现状

现有项目看板（`web-platform/src/handlers/kanban.rs`、`web-frontend/src/components/kanban/KanbanBoard.tsx`）是按实现进度分列：

- Todo：没有进行中 label 的 Issue；
- In Progress：`symphony-claimed`、`In Progress`、`Merging`、`Rework`；
- Testing：项目开启测试时才显示；
- PR：打开的 MR。

### 9.2 新分列

构建和测试合为一列。原因有两个：测试阶段是可选的（`testing_enabled`），teamwork 模式还会去掉 `Testing`；而且这两个阶段都由 agent 在同一个工作区里连续完成。

| 列 | Playbook 阶段 | 包含的 label（状态） | 卡片上的子状态标签 |
|---|---|---|---|
| 计划 | Plan | 无流程 label、`Backlog`、`doc:intent`、`doc:intent-review` | 待启动 / 起草 intent / 待审 intent |
| 设计 | Design | `doc:spec`、`doc:spec-review` | AI 起草 spec / 待审 spec |
| 构建与测试 | Build + Test | `doc:plan`、`doc:plan-review`、`Todo`、`symphony-claimed`、`In Progress`、`Rework`、`Testing` | AI 起草 plan / 待审 plan / 已批准待合并 / 排队 / 实现中 / 返工 / 测试中 |
| 部署 | Deploy | `Human Review`、`Merging`，以及关联的代码 MR | 待审 MR / 合并中 |
| 运维 | Maintain | 最近 N 天关闭、且不是被拒绝的 Issue（默认 7 天，可配置）；P3 起增加“待分诊信号” | 已上线 / 待分诊 |

说明：

- **plan.md 放在“构建与测试”列**。Playbook 里 plan mode 属于 Build 阶段，“计划”列对应的是 Plan 阶段的 intent。命名上容易混淆，列标题下用小字注明“intent”“spec”“plan → 实现 → 测试”。
- **`track:fast` 的 Issue** 不写文档，打 `Todo` 后直接出现在“构建与测试”列。
- **MR 卡片**：
  - 代码 MR 继续显示在“部署”列，挂在对应 Issue 下。
  - 文档 MR（分支前缀 `symphony/docs-`）不进“部署”列，以链接形式显示在 Issue 卡片上，避免和代码 MR 混在一起。
- **“运维”列第一版的内容**：只展示最近完成的工作。P3 接入外部信号后，由信号生成的 intent 进入“计划”列（`doc:intent-review`），“运维”列显示还没生成 intent 的待分诊信号。“运维”列显示近期完成工作，这一点是对 Playbook 的推断，原文没有规定看板形式。

### 9.3 卡片信息

- **子状态标签**：显示当前所处的具体状态，即上表最右一列。
- **等人标记**：处于 `*-review`、`Human Review` 的 Issue 标明“等待审批”和已等待时长；agent 运行中的 Issue 标明“AI 运行中”。
- **文档进度**：卡片上显示 intent / spec / plan 三个小图标，分别表示未开始、草稿、待审、已批准、stale。
- **路径**：`track:full` / `track:plan` / `track:fast` 用小标签显示。

### 9.4 实现要点

- **映射表**：后端维护一张“label → 列 + 子状态”映射表，项目看板和全局看板共用。第一版写死在代码里，以后再改为可配置。
- **查询方式**：沿用现有“按 label 分批查询再合并去重”的方式（`in_progress_issue_labels` 的做法）。“运维”列额外查询最近关闭的 Issue。
- **接口兼容**：响应结构从固定的 `todo` / `in_progress` / `testing` / `pr` 字段，改为按列返回的数组。前端看板组件随之改为按列渲染，不再写死列数。
- **一个 Issue 只在一列**：如果 Issue 同时带了多个流程 label（异常情况），按“越靠后的阶段越优先”取一列，并在卡片上提示 label 异常。

## 10. 端到端运转示例与文档 MR 合并

以 Issue #123“理赔进度自助查询”、`track:full` 为例。

### 10.1 账号约定

| 操作 | 使用的身份 |
|---|---|
| 文档分支 commit / push、开文档 MR、写 label 和评论 | 项目**已配置的 Token**，下称“系统账号” |
| 合并文档 MR | 执行 G3 批准的人自己的平台 Token（`user_config` 中的个人 Token，沿用 web-platform 现有做法） |

**事实说明**：“已配置的 Token”不是独立的机器人账号。`web-platform/src/process_manager/spawn.rs` 启动编排器时读取的是项目创建者（`project.created_by`）的个人 Token，所以文档 MR 在平台上的作者就是项目创建者。

**风险**：所有文档操作都依赖这个 Token。创建者 Token 失效、权限变更或人员离职时，整个项目的文档流程中断。Token 校验失败时，在项目设置和卡片上提示“项目 Token 不可用”。

**人和 AI 的区分**（已确定，只区分这两类）：每个文档 commit 加 trailer。

```text
Symphony-Actor: ai        # AI 起草或修改
Symphony-Actor: human     # 人在工作台修改
```

平台上的 commit 作者都显示为 Token 主人，审计时以 trailer 为准。具体是哪个人批准的，以 `document_approvals.actor` 为准。

**第一期允许自己合并自己的 MR**（已确定）：为了让核心工程师快速验证流程，G3 批准人即使就是文档 MR 的作者（项目创建者），也可以直接合并。

- Symphony 的 G3 批准与平台的 MR 审批分开。审计意义上的批准人以 Symphony 记录为准，平台审批只为通过分支保护。
- Symphony 合并前不额外调用平台“批准”接口，直接调用合并接口。
- 风险：主干分支保护如果要求至少 1 个审批，或者 GitLab 开启了“禁止作者批准”，作者自己合并会被平台拒绝。第一期的目标仓库需要不要求文档 MR 审批，或给核心工程师绕过权限；被拒绝时，卡片提示“平台要求另一位成员审批”并附 MR 链接。
- 第一期、第二期都不启用“提交审阅的人不能批准同一份文档”（第 11 节）。如果以后要收紧，增加该约束和平台审批步骤即可，数据模型不变。

### 10.2 运转步骤

**第 1 步：起草 intent（计划列）**

1. 提出者在 Symphony 点“新建需求”（文档流程只从界面进入，见第 2 节），用一句话描述。快速生成入口按 intent 模板生成 `intent.md` 草稿。
2. Symphony：
   - 创建 Issue，打上 `track:full`、`doc:intent`，Issue 正文只放摘要和文档链接；
   - 从主干切出 `symphony/docs-123`，写入 `docs/changes/123/intent.md`；
   - 用系统账号提交、推送，开文档 MR（Draft）。
3. 提出者在工作台和 AI 完善 intent。AI 读代码、追问，提出者回答后 AI 修改。自动保存只写工作区文件；点“保存”或 AI 一轮结束时才在文档分支上 commit 并 push。
4. 提出者点“提交审阅”，模板校验通过后 intent 进入 `in_review`，label 改为 `doc:intent-review`。

**第 2 步：G1 审 intent**

5. 审阅人在工作台按审阅清单审阅，可直接修改，然后批准。
6. Symphony 记录批准（批准人、blob SHA、commit SHA），在文档 MR 和 Issue 上评论“G1 已批准 intent.md @ <SHA>，批准人 <姓名>”，label 改为 `doc:spec`。**文档 MR 不合并。**
7. 拒绝时：intent 变为 `rejected`，关闭 Issue 和文档 MR，主干不留任何文件。

**第 3 步：起草 spec，G2 审 spec（设计列）**

8. G1 批准后，AgentSessionService 立即在文档工作区启动非交互 Codex 会话：加载组织 skills，只能写 `docs/changes/123/`，读取已批准的 intent 和代码，按模板写出 `spec.md` 并标出关注点；完成后自动提交审阅，label 改为 `doc:spec-review`。
9. 审阅人在工作台审 spec，可让 AI 修改；关注点逐条处理完，“批准”才可点。批准后 label 改为 `doc:plan`。
10. 级联：审 spec 时如果回头改了 intent，intent 批准作废，Issue 退回 `doc:intent-review`；intent 重新批准后，spec 变为 stale，需要重审（第 4.3 节）。

**第 4 步：起草 plan，G3 审 plan（构建与测试列）**

11. G2 批准后，AgentSessionService 起 plan 会话：读代码，写出 `plan.md`（改哪些文件、顺序、证明测试、风险、放弃的方案），label 改为 `doc:plan-review`。
12. 工程师在工作台追问 AI（可能弄坏什么？哪一步风险最大？），迭代到满意。

**第 5 步：G3 批准，文档 MR 合入主干**

13. 工程师点“批准”。三份文档都已批准，Symphony 依次执行：

```text
a. 记录 G3 批准（批准人、plan.md SHA）
b. 文档 MR 从 Draft 转为 Ready（系统账号）
c. 不 rebase：直接检查能否合并
   每个 Issue 有独立目录，冲突几乎不会发生；平台报告冲突时，把主干 merge 进文档分支（不用 rebase），仍冲突则转人工
d. 等待主干要求的 CI 通过
e. 用 G3 批准人的个人 Token 合并文档 MR（第一期允许作者自己合并，见 10.1）
   合并方式：merge commit，不用 squash
f. 合并成功后：三份文档进入 merged 阶段（4.2）；label 改为项目 WORKFLOW 中 Todo 对应的 label；在 Issue 上记录“文档已合入主干 @ <merge commit SHA>”
```

合并规则：

- **不 rebase、不 squash**：merge commit 保留文档分支上的每个原始 commit，数据库里记录的批准 commit SHA 在主干历史中都能找到；即使出现例外，批准也以 blob SHA 为准。
- **合并失败不推进状态**：CI 失败、冲突、批准人 Token 无合并权限时，plan 保持已批准，Issue 停在 `doc:plan-review`。卡片显示“已批准，待合并”和失败原因，提供“重试合并”按钮。只有合并成功才进入 Todo。
- **批准前预检**：G3 批准人没有配置个人 Token，或在平台上没有合并权限时，“批准”按钮禁用并说明原因，不等批准后才失败。提示文案与现有看板、MR 接口一致。
- **CI（已确认）**：目标仓库 CI 对文档 MR 不跑耗时检查，第 d 步只等待很快结束的检查。注意：如果平台开启了“流水线必须成功才能合并”，而文档 MR 完全不触发流水线，部分平台会挡住合并；遇到时给文档 MR 配一条立即通过的空任务即可。

**第 6 步：实现与测试（构建与测试列，现有流程）**

14. 编排器看到 Todo：从已包含三份文档的主干切实现分支；提示词注入三份文档路径和 SHA，要求先读 `plan.md`；Codex / teamwork 实现、自测；偏离计划时在同一个 commit 更新 `plan.md`。
15. 发布前：确定性检查 diff 文件与 plan.md 文件清单，不一致时在代码 MR 上标注；审查增加对照 spec / plan 的 pass。

**第 7 步：代码审查与合并（部署列）**

16. 代码 MR 进入 `Human Review`，code owner 审查。代码 MR 只含代码和实现期的 plan.md 修订，文档问题和代码问题分开审。
17. 审查结果：
    - 批准 → `Merging` → 现有 land 流程 → `Done`；
    - 代码问题 → `Rework`，与现在一致；
    - 需求或计划问题 → 由人在 Symphony 主动点“退回文档阶段”，选择退回 `doc:spec-review` 或 `doc:plan-review`。Symphony 依次：
      1. 停止实现阶段的 agent（Issue 离开活跃状态）；
      2. 代码 MR 保持打开，转为 Draft，回到实现阶段后继续使用；
      3. 从主干切出 `symphony/docs-123-r2`，以**实现分支上的最新文档**为准复制过去（包含实现期对 plan.md 的修订），开第二个文档 MR；revision 记为 r2，merged 之前的规则（4.2、4.3）重新生效；
      4. 重新通过 G3、r2 合入主干后，实现分支 merge 新主干再继续。`docs/changes/<iid>/` 下的冲突一律以主干（r2 已批准版本）为准。

**第 8 步：运维列**

18. Issue 关闭后（非拒绝）在“运维”列显示 7 天。P3 接入外部信号后，告警写出新的 intent，从第 1 步重新开始。

### 10.3 不同路径的文档 MR

| 路径 | 文档 MR |
|---|---|
| `track:full` | 包含 intent、spec、plan，G3 通过后合入 |
| `track:plan` | 只有 plan.md，从第 4 步开始，G3 通过后合入 |
| `track:fast` | 没有文档 MR，直接从第 6 步开始 |

## 11. 决策记录

### 已确定

| 决策 | 结论 |
|---|---|
| 审批状态存放 | 只放数据库，文件中不写 |
| 上游变更后的下游处理 | 自动置为 stale 并退回 Review |
| 模板范围 | 第一期不做变体，只做 intent、spec、plan 三类模板 |
| 1. 文档合入主干时机 | B：一个文档 MR，G3 通过后合入 |
| 2. 闸门审批人 | 第一版任何项目成员都能批准，记录具名批准人（第 6 节） |
| 看板分列 | 按计划、设计、构建与测试、部署、运维分列，只做展示映射，不影响 label（第 9 节） |
| 3. 路径分级 | `track:full` / `track:plan` / `track:fast`；在 Symphony 界面启动文档流程时默认选中 `plan`；“新建需求”创建的 Issue 走 `full`；`fast` 只开放给白名单 label；路径只能升级 |
| 4. 状态表示 | 新增 6 个带前缀的 label：`doc:intent`、`doc:intent-review`、`doc:spec`、`doc:spec-review`、`doc:plan`、`doc:plan-review`；只由 Symphony 写入；人工修改不自动改回，只提示（4.1） |
| 5. 入口 | 保留快速生成入口，输出改为 intent.md 草稿并进入工作台；Issue 正文只保留摘要和链接；`fast` 路径仍直接写 Issue 正文 |
| 文档 MR 合并 | 系统账号（项目已配置 Token，即项目创建者的 Token）开 MR 和推送；G3 批准人用个人 Token 合并；第一期允许作者自己合并；不 rebase、用 merge commit；合并成功才进入 Todo（第 10 节） |
| 人和 AI 的区分 | 只区分两类，用 commit trailer `Symphony-Actor: ai/human`（10.1） |
| 批准后的修改 | 文档 MR 合并后进入 merged 阶段，不再比对 SHA、不级联；改需求或计划须人工退回并开 r2 文档 MR（4.2） |
| 批准绑定 | 以文件 blob SHA 为准，commit SHA 只作参考（4.2） |
| 起草 agent 的运行方 | web-platform 的 AgentSessionService；编排器不调度 `doc:*`（4.7） |
| 文档流程入口 | 只从 Symphony 界面进入（“新建需求”或“开始文档流程”）；平台上直接创建的 Issue 不自动起草（第 2 节） |
| 人工修改 label | 不监听、不自动改回，只提示不一致；手动改为 Todo 视为 `track:fast`（4.1） |
| 文档目录 | 固定 `docs/changes/<iid>/`，不可配置；项目级模板在 `docs/changes/_templates/`（第 3 节） |
| Codex 认证 | 第一期沿用编排器的 `CODEX_HOME`，不建独立目录（工作台方案 4.2） |
| 第一期分支保护 | 已确认：目标仓库文档 MR 不要求平台审批，或核心工程师有绕过权限，保证作者可自合并 |
| 平台范围 | 第一期 GitLab / GitHub；Gitea、Linear 不支持文档闸门（2026-10-27 修订，原为含 Gitea） |
| frontmatter | 只在创建时写一次；计算 blob SHA 前去掉 frontmatter |
| plan 文件清单 | plan 模板固定 `## Files` 小节，供偏离检测使用 |
| 提交人自批限制（M1） | 第一期、第二期都不启用：提交审阅的人可以批准自己的文档。审计仍记录具名批准人 |
| 上线迁移（M4） | 不做自动迁移。上线前已有的 Issue 由人工处理；系统不对存量 Issue 做 label 转换 |
| CI 检查 | 目标仓库 CI 对文档 MR 不跑耗时检查（10.2 第 5 步） |
| 工作台：AI 写入审批 | 白名单内自动批准，不逐次确认，本轮结束后看 diff（工作台方案 4.2） |
| 工作台：会话划分 | 同一阶段共用会话，跨阶段新开会话，以已批准文档交接（工作台方案 4.2） |
| 工作台：编辑权 | 不做编辑权分配；AI 运行期间本轮修改的文件只读（工作台方案 4.4） |
| 工作台：从仓库 | 多仓库项目的从仓库只读完整检出到 `<alias>/`（布局与实现阶段工作区一致），供 AI 写 spec / plan 时读取；实现阶段的可写性仍由项目多仓库配置决定（工作台方案 4.1） |
| 组织规则仓库 | `codex-org-config` 第一期不实现，只记录需求（[codex-org-config-requirements.md](./codex-org-config-requirements.md)），建议 P2 |
| incident 模板 | 放到 P3：作为事故来源的 intent 模板变体（在 intent 骨架上增加“异常与证据”，换用分诊审阅清单），随外部信号接入一起做；第一期只有 intent、spec、plan 三类模板 |

### 仍需确认

无。全部决策已确定。

### 决策说明（已确定，保留供追溯）

每项按“要决定什么 → 为什么重要 → 选项对比 → 推荐与理由 → 以后能否改”说明。决策 2 的最终结论与推荐不同，以“已确定”表为准。

#### 决策 1：文档什么时候合入主干

**要决定什么**：intent / spec / plan 在文档分支上写完、批准后，什么时候进入主干（main）。

**为什么重要**：

- 实现阶段的 agent 从主干切分支。主干上有没有已批准的 plan.md，决定实现分支能不能直接读到它。
- 主干的 git 历史是审计链的一部分。Playbook 原文是“接受即合并、拒绝即关闭”，把合并当作批准的证据。
- 被拒绝的 intent 如果进了主干，会留下噪音文件。

**选项对比**：

| | A. 每道闸门合并一次 | B. 一个文档 MR，G3 通过后合入 | C. 文档和代码同一个 MR |
|---|---|---|---|
| 每个 Issue 的 MR 数 | 3 个文档 MR + 1 个代码 MR | 1 + 1 | 1 |
| 和 Playbook 的贴合度 | 最高，合并即批准 | 中：中间闸门的批准记录在 Symphony 和 MR 评论里，不在 git 合并记录里 | 低：代码合并前，主干上没有任何文档 |
| 被拒绝的 intent | intent 已合入主干，要再提交一次标记关闭 | 不进主干，直接关闭 MR | 不进主干 |
| 实现分支怎么拿到 plan | 从主干切出即可 | 从主干切出即可 | 在文档分支上继续开发 |
| 审查体验 | 每个 MR 内容单一，但 MR 数量多 | 文档审查和代码审查分开 | 一个 MR 里文档和代码混在一起，打回时难判断是哪一层的问题 |
| 并发风险 | 多个 Issue 的文档 MR 频繁合并，冲突少（每个 Issue 有独立目录） | 同 A | 无 |
| 实现复杂度 | 高：每道闸门都要自动合并，还要处理 CI 和分支保护 | 中：只在 G3 合并一次 | 低 |

**推荐 B**：

- 主干上只出现“已经可以开工”的完整文档链，实现分支直接可用。
- 审批证据的完整性由 Symphony 数据库加 MR 评论保证，每次批准都带 SHA，不依赖 git 合并记录。
- MR 数量可控，主干分支保护只需处理一次文档合并。

**以后能否改**：能。B → A 只是把“G3 合并”拆成三次合并，数据模型不变。

**补充问题**：主干如果有分支保护、要求 MR 审批，文档 MR 由谁批准合并？可以由 G3 的批准人顺带批准，也可以给文档目录单独配置 code owner。（已解决，见第 10.1 节）

#### 决策 2：闸门由谁批准

**要决定什么**：每道闸门的批准人怎么配置，一个人能否兼任多道闸门。

**为什么重要**：

- Playbook 要求每道闸门由**具名的人**负责，这是审计链里“谁批准的”。
- 团队小的话，严格分角色会导致所有工作卡在一两个人身上。
- Symphony 现在只有项目成员角色（`web-platform/src/models/project.rs` 的 `role`），没有“产品负责人”“政策负责人”这类概念，需要新增配置。

**选项对比**：

| | A. 按项目成员角色 | B. 每道闸门单独配置审批人名单 | C. 复用 Git 平台的 CODEOWNERS |
|---|---|---|---|
| 配置方式 | 例如 maintainer 可以批准所有闸门 | 项目设置里为 G1 / G2 / G3 分别选人 | 把规则写在仓库的 CODEOWNERS 里 |
| 灵活度 | 低，无法区分产品和工程 | 高 | 中，只能按路径区分，不能按闸门区分 |
| 实现成本 | 最低 | 中 | 高，三个平台规则不同 |
| 适合 | 很小的团队 | 大多数团队 | 已经大量使用 CODEOWNERS 的团队 |

**推荐 B**，并约定以下规则：

- 每道闸门至少配置 1 人，可以配置多人，任意一人批准即可。
- **允许兼任**，但有一条硬约束：**发起人不能批准自己发起的文档**。G1 批准人不能是 intent 的作者，G3 批准人不能是触发 plan 生成的人。团队小时，这条可以在项目设置里关闭，但关闭操作要记入审计日志。
- 政策负责人不作为固定闸门，而是按 spec 里的关注点类型（安全、合规、UX）点名会签。关注点未全部处理，G2 不能批准。

**需要你提供**：你们团队里谁担任产品负责人、技术负责人，有没有专门的安全或合规负责人。

**以后能否改**：能。审批人名单是项目配置，随时可改，历史批准记录不受影响。

#### 决策 3：所有工作都要走完三道闸门吗

**要决定什么**：按工作大小分几条路径，没打 label 的 Issue 默认走哪一条。

**为什么重要**：

- 三道闸门对新功能有价值；对改一行配置、修一个拼写错误来说，是纯粹的负担，团队会绕开流程。
- Playbook 原文也说明，小而有界的修复可以直接开 PR 进入审查闸门，不一定回到 Plan。
- 默认值决定了大部分工作的实际体验。

**选项对比**：

| 路径 | 走哪些闸门 | 适合 | 风险 |
|---|---|---|---|
| `track:full` | G1 → G2 → G3 → 实现 | 新功能、跨模块改动、涉及安全或合规 | 小改动会被拖慢 |
| `track:plan` | 只有 G3 → 实现 | 需求已明确的技术改动、重构、中等 bug | 需求层面的问题要到 Human Review 才暴露 |
| `track:fast` | 直接 Todo（现有流程） | hotfix、文案、依赖升级、小 bug | 没有计划可对照，审查只能看 diff |

**默认值的选择**：

| 默认 | 优点 | 缺点 |
|---|---|---|
| 默认 `full` | 最符合 Playbook，能充分体验新流程 | 上线初期大量小工作被拖慢，团队容易抵触 |
| 默认 `plan`（推荐） | 每件工作至少有一份被人接受的计划，“没有被接受的计划就不实现”这条最关键的规则始终成立；成本适中 | 新功能需要手动选 `full` |
| 默认 `fast` | 和现在完全一样，零阻力 | 新流程只在主动选择时生效，很难推广 |

**推荐默认 `plan`**（已修订：“默认”指在 Symphony 界面启动文档流程时默认选中 `plan`，平台上直接创建的 Issue 不自动进入文档流程，见第 2 节和审阅 A3），同时建议：

- 从工作台写 intent 创建的 Issue，自动走 `full`。
- `fast` 需要显式打 label，并且只允许 `hotfix`、`docs-only` 等现有 label 或配置的白名单使用。这与现有模板里 Testing Gate 跳过测试的 label 判断方式一致。
- 路径可以升级不能降级：`plan` 中途发现需求不清，可以升级为 `full`，补写 intent 和 spec。

**以后能否改**：能。默认路径是项目配置。

#### 决策 4：新增的流程状态用 label 表示

**要决定什么**：Intent、Intent Review、Spec、Spec Review、Plan、Plan Review 这 6 个新状态，怎么在 GitHub / GitLab 上表示。

**为什么重要**：

- 现有编排器靠 Issue label 判断状态（`tracker.active_states` 和 `workflow_labels`，代码在 `rust-platform/src/config/service_config.rs`，平台适配在 `rust-platform/src/platform/*.rs`）。
- 在 Git 平台上看 Issue 的人，需要知道这件工作卡在哪一步。

**选项对比**：

| | A. 新增 6 个 label | B. 只加 1 个 `Docs` label，细分状态只在 Symphony 内部 | C. 用平台的 Issue 看板列或自定义字段 |
|---|---|---|---|
| 和现有机制的一致性 | 完全一致，沿用 label 切换 | 部分一致 | 不一致，三个平台各不相同 |
| 在 Git 平台上能否看清进度 | 能 | 只能看到“在写文档” | 能，但各平台体验不同 |
| 编排器改动 | 小：把新 label 加入 `workflow_labels` 和状态路由 | 小 | 大：要接入看板或字段 API |
| 缺点 | label 列表变长；Linear tracker 需要单独映射 | 在 Git 平台上看不出卡在哪道闸门 | 实现成本高，Gitea 支持有限 |

**推荐 A**：

- label 名称加统一前缀（如 `doc:intent-review`），和现有状态 label 区分开，在平台上筛选也方便。
- 按 4.5 节的规则，label 只由 Symphony 根据文档状态写入。人手动改 label 不会生效，Symphony 会按文档状态改回来，并在 Issue 上留言说明。这样 label 只是“显示”，不会和数据库状态冲突。（已修订：最终不自动改回，只提示不一致，见第 4.1 节和审阅 A3。）
- 同步调整：看板按第 9 节分列；告警规则（如任务超时）区分“等人审批”和“agent 运行中”，等人审批的状态不触发超时告警。

**以后能否改**：可以改名，但已有 Issue 的 label 需要迁移，所以 label 名称最好第一次就定好。

#### 决策 5：新需求从哪里开始

**要决定什么**：现有的“AI 生成 Issue”（`web-platform/src/handlers/ai_generate.rs`，一次性生成 Issue 正文）要不要改成“在工作台和 AI 一起写 intent.md”。

**为什么重要**：

- 入口决定 intent 是不是唯一的需求来源。如果需求同时写在 Issue 正文和 intent.md 里，两边很快会对不上，Playbook 也要求每类产物只有一个事实来源。
- 现有入口生成时读不到代码；工作台入口可以让 AI 读代码、追问提出者，这正是 Playbook 里“Claude 会问分析师会问的问题”的体验。

**选项对比**：

| | A. 替换现有入口 | B. 两个入口并存 | C. 保留现有入口，生成后转成 intent 草稿 |
|---|---|---|---|
| 需求事实来源 | 只有 intent.md | Issue 正文和 intent.md 两份 | 只有 intent.md |
| 用户习惯 | 改变较大 | 不变 | 基本不变 |
| 能否在写需求时读代码、追问 | 能 | 只有新入口能 | 转成草稿后，可以在工作台继续 |
| 实现成本 | 中 | 低 | 中 |

**推荐 C**，作为过渡：

- 保留现有的“快速描述 → AI 生成”入口，但输出改为按 intent 模板生成 `intent.md` 草稿，生成后直接进入工作台继续修改。
- Issue 正文只保留摘要和文档链接，正文由 Symphony 根据 intent 自动同步，标明“以 intent.md 为准”。
- `track:fast` 的工作不写 intent，仍然用现有方式直接写 Issue 正文。
- 稳定后再评估是否完全替换为 A。

**以后能否改**：能。C 到 A 只是去掉快速生成入口。

#### 决策之间的依赖

- 决策 3 的 `track:fast` 会影响决策 5：fast 路径不写 intent，Issue 正文仍是需求来源。
- 决策 1 选 A 时，决策 2 中“文档 MR 由谁批准合并”的问题会出现三次，要一并考虑。
- 决策 4 的 label 名称一旦启用就不宜再改，建议最后和决策 3 的路径 label 一起定名。

## 12. 分期

| 阶段 | 内容 |
|---|---|
| P1 | 文档闸门、文档状态与级联、三类模板、6 个文档 label、实现阶段读取 plan.md、看板按阶段分列。**前置条件**：工作台方案 M1–M3 完成；M4 可与 P1 并行 |
| P2 | 对照 spec / plan 的审查、计划偏离检测、度量、闸门角色与审批人配置（不含提交人自批限制）、组织规则仓库 codex-org-config（建议） |
| P3 | 外部信号 → intent（含事故来源的 intent 模板变体）、运维列待分诊信号、组织配置 eval |

生产发布由目标项目 CI/CD 负责，不纳入 Symphony。
