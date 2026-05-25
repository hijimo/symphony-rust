# ADR-004: WORKFLOW.md 作为单一配置源

## 状态

accepted

## 上下文

Symphony 的 Agent 行为由多种配置共同决定：
- Tracker 连接参数（平台类型、API Token、项目 ID）
- 轮询策略（间隔、活跃状态列表）
- 并发控制（最大并发数、按状态并发上限）
- 重试策略（退避基数、上限）
- 工作空间配置（根目录、GC 策略）
- 生命周期 Hook 脚本（`after_create`、`before_run`、`after_run`、`before_remove`）
- Agent Prompt 模板（首轮指令、续行指令）

这些配置需要满足以下要求：
- **团队可审查**：配置变更应该能通过 Pull Request 审查，而不是只有管理员能看到
- **版本管理**：配置历史可追溯，可以回滚到任意历史版本
- **环境一致性**：开发、测试、生产环境使用相同的配置文件，减少环境差异
- **热重载**：配置变更无需重启服务即可生效
- **Prompt 与配置同源**：Agent 的行为策略（Prompt）和运行时配置（轮询间隔等）应该在同一个地方定义，避免分散

## 决策

使用 **WORKFLOW.md 作为单一配置源**，采用 YAML Front Matter + Markdown 正文的格式：

```
---
# YAML Front Matter：结构化运行时配置
tracker:
  type: linear
  api_key: $LINEAR_API_KEY
  team_id: $LINEAR_TEAM_ID
  active_states: ["Todo", "In Progress"]

agent:
  max_concurrent: 3
  stall_timeout_secs: 300

hooks:
  after_create: "git clone $REPO_URL ."
  before_run: "git pull origin main"
---

# Markdown 正文：Liquid Prompt 模板

你是一个自动化编码代理，负责处理以下 Issue：

**Issue**: {{ issue.title }}
**描述**: {{ issue.description }}

请完成编码任务并提交 Pull Request。
```

配置加载流程：
1. `workflow_loader` 解析 YAML Front Matter → `WorkflowDefinition`
2. `service_config` 将 `WorkflowDefinition` 转换为类型化的 `ServiceConfig`，解析 `$ENV_VAR` 引用
3. `validator` 校验配置完整性和合法性
4. `prompt` 模块编译 Markdown 正文为 Liquid 模板

敏感信息（API Token、密钥）通过 `$ENV_VAR` 语法引用环境变量，不直接写入文件，可以安全地提交到版本库。

## 备选方案

### 方案 A：独立 YAML 配置文件 + 独立 Prompt 文件

将结构化配置（`config.yaml`）和 Prompt 模板（`prompt.md`）分为两个独立文件。

未采用原因：
- 配置和 Prompt 紧密相关（如 `active_states` 影响 Prompt 中的状态描述），分离后需要在两个文件间保持一致
- PR 审查时需要同时查看两个文件，增加认知负担
- 文件数量增加，项目结构更复杂

### 方案 B：数据库存储配置

将所有配置存储在 web-platform 的 SQLite 数据库中，通过 Web UI 管理。

未采用原因：
- 配置不在版本库中，无法通过 PR 审查配置变更
- 无法追溯配置历史（除非额外实现审计日志）
- 不同环境（开发/生产）的配置同步困难
- 热重载需要额外的轮询或推送机制

### 方案 C：纯环境变量配置

所有配置通过环境变量传递，Prompt 通过单独的环境变量或文件传递。

未采用原因：
- 环境变量不适合存储多行文本（Prompt 模板）
- 配置项多时环境变量管理混乱
- 无法版本管理（除非将 `.env` 文件提交到版本库，但这与安全实践相悖）
- 热重载需要重启进程

### 方案 D：TOML 配置文件

使用 TOML 格式的配置文件，Prompt 作为多行字符串字段。

未采用原因：
- TOML 多行字符串语法不适合编写长篇 Prompt，可读性差
- 无法利用 Markdown 的格式化能力（标题、列表、代码块）来组织 Prompt 内容
- 团队成员对 Markdown 更熟悉，降低了配置编写门槛

## 后果

正面影响：
- **单文件审查**：配置变更通过 PR 提交，团队成员可以在同一个文件中审查运行时配置和 Prompt 策略
- **版本可追溯**：`git log WORKFLOW.md` 可以查看所有配置变更历史
- **热重载**：`notify` crate 监听文件变更，`ArcSwap` 原子替换配置，无需重启服务
- **环境一致性**：同一份 WORKFLOW.md 在所有环境中使用，通过环境变量注入敏感信息
- **Prompt 即文档**：Markdown 格式的 Prompt 模板可读性好，便于团队理解和修改 Agent 行为
- **平台适配**：项目提供多个平台变体（`WORKFLOW.md.github`、`WORKFLOW.md.gitlab`、`WORKFLOW.md.Linear.md`），团队可按需选用

负面影响：
- **文件依赖**：配置必须以文件形式存在，不适合纯容器化或无文件系统的部署场景
- **格式耦合**：YAML Front Matter + Markdown 是非标准格式，需要自定义解析器
- **Prompt 调试**：Liquid 模板语法错误只在运行时发现，缺乏静态检查
- **多项目管理**：每个项目需要独立的 WORKFLOW.md，多项目场景下文件管理略显繁琐（web-platform 通过项目配置管理各项目的 WORKFLOW.md 路径来缓解此问题）

已接受的权衡：
- 文件依赖限制是可接受的，Symphony 的目标场景是单机部署，文件系统始终可用
- 格式耦合通过完善的解析器和错误提示来缓解

## 日期

2024-01-01
