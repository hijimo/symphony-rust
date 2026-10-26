# 组织规则仓库 codex-org-config 需求记录

> **状态**：需求记录，**第一期不实现**，排期待定（建议 P2）。
>
> **关联方案**：
> - [ai-doc-workbench-design.md](./ai-doc-workbench-design.md) 第 4.2 节“平台统一管理的 CODEX_HOME”
> - [ai-native-sdlc-flow-design.md](./ai-native-sdlc-flow-design.md) 第 5.2 节模板优先级、第 8 节规则与知识
>
> `codex-org-config` 是方案中的暂定名称，仓库目前不存在。

## 1. 要解决的问题

本地 Codex 会读取两层规则：仓库里的 `AGENTS.md` / skills，以及个人电脑上的 `~/.codex/`（全局 `AGENTS.md`、skills、`config.toml`）。

部署在服务端的 Symphony 只能读到仓库级规则，读不到任何人的 `~/.codex/`。因此跨项目通用的组织规则（安全、合规、UX、品牌规范，文档写法）在网页端没有来源，Playbook 要求的“写 spec 时就应用组织政策”无法做到。

## 2. 需求

### 2.1 仓库内容

```text
codex-org-config/
├── AGENTS.md            # 组织级通用规则
├── config.toml          # 默认模型、推理强度、审批默认值、MCP 配置（不含任何密钥）
├── skills/
│   ├── security-policy/ # 示例：接口鉴权、日志脱敏
│   ├── ux-standards/    # 示例：表单、错误提示、无障碍
│   └── symphony-docs/   # intent / spec / plan 写法，本地编辑器也可安装
└── templates/           # 组织级文档模板
    ├── intent.md
    ├── spec.md
    └── plan.md
```

### 2.2 功能需求

| 编号 | 需求 |
|---|---|
| R1 | 规则变更走 PR，合并后才对 Symphony 生效 |
| R2 | Symphony 把仓库同步到服务端目录，作为启动 Codex app-server 时的 `CODEX_HOME`；不读运行账号自己的 `~/.codex` |
| R3 | 同步方式支持定期拉取或按版本（tag / commit）固定，项目可选择跟随最新或锁定版本 |
| R4 | 每个 AI 会话和每份生成的文档记录当时的组织配置 commit SHA（`org_config_sha`），可追溯文档在哪套规则下生成 |
| R5 | 组织级模板参与模板优先级：内置默认 → 组织级 → 项目级（`.symphony/templates/`） |
| R6 | 项目可追加自己的 skills，优先级高于组织级 |
| R7 | 同一套 skills 可安装到本地 `~/.codex/skills/`，让本地 Codex / Kiro 与网页端使用相同规则和模板 |
| R8 | 项目设置页显示当前生效的组织配置版本和来源 |

### 2.3 约束

- 仓库中不得出现任何密钥、Token 或凭据。模型和 MCP 凭据由部署环境注入，也不传给工作区内执行的命令。
- 同步失败时继续使用上一次成功同步的版本，并在界面和日志中提示，不阻断文档流程。
- 组织规则视为数据输入，Symphony 不执行仓库中的脚本。

### 2.4 后续扩展（P3）

- 仓库 CI 用 20–50 个真实任务跑 eval，配置或模型变更导致通过率下降时，先审再合并。

## 3. 第一期的替代做法

第一期不建仓库、不做同步，流程照常运行：

| 方面 | 第一期做法 |
|---|---|
| 文档模板 | 只用两层：Symphony 内置默认模板 → 项目级 `.symphony/templates/` |
| 规则 | 只用目标仓库自带的 `AGENTS.md` 和仓库内 skills |
| `CODEX_HOME` | 仍使用平台管理的独立目录（只放最小 `config.toml`），不读运行账号的 `~/.codex`，避免个人配置混入 |
| `org_config_sha` | 字段保留，第一期为空 |
| 已知限制 | Spec 起草时不会应用组织级安全、合规、UX 政策，这类问题要到 Human Review 才可能被发现 |

## 4. 待决定（实施前）

1. 仓库名称和托管位置（GitHub / GitLab）。
2. 规则内容的维护人：业务规范（安全、UX、品牌）由谁撰写，谁负责改写成 skill 格式。
3. 首批纳入的 skills 范围。建议先放 `templates/` 和 `symphony-docs`，其余逐步补充。
