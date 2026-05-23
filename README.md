# Symphony

Symphony 是一个 Rust 实现的自动化编码代理编排平台。当前仓库包含两部分：

- `web-platform`：Web 管理后台 API，负责用户、项目、成员、看板、Issue、AI Issue 生成、并发控制和告警等能力。
- `rust-platform`：项目级编码代理运行时，负责读取项目 `WORKFLOW.md`、拉取 Issue、启动 Codex、管理工作空间和回写平台状态。

前端在 `web-frontend`，使用 React + Vite，对接 `web-platform` 提供的 `/api` 接口。

> Symphony 仍处于工程预览阶段。开发环境可以使用 `dev.sh` 快速启动；生产环境请使用独立数据库、强随机密钥和正式的密钥管理方式。

## 功能概览

- 项目管理：创建 GitHub/GitLab 项目，维护工作流模板和成员权限。
- Web 控制台：项目列表、项目详情、Issue/MR 查看、服务启停、并发状态和告警配置。
- 平台适配：支持 GitHub/GitLab Issue 与 MR 工作流。
- Codex 编排：为每个任务创建独立工作空间，按项目配置启动编码代理。
- AI Issue 生成：后端调用 Azure OpenAI 或 OpenAI-compatible `/v1` 接口，通过 SSE 流式返回 Issue 内容。
- 运维能力：SQLite 存储、JWT 登录、Token 加密、进程管理、告警指标和通知渠道。

## 目录结构

```text
.
├── dev.sh                 # 本地一键启动 Web 后端和前端
├── web-platform/          # Rust Web API 服务
├── web-frontend/          # React + Vite 前端
├── rust-platform/         # Codex 编排运行时
├── docs/                  # 阶段设计、API 规格和测试文档
└── Cargo.toml             # Rust workspace：web-platform + rust-platform
```

## 环境要求

- Rust 1.70+，推荐使用当前 stable。
- Node.js 20+ 和 npm。
- Codex CLI，用于 `rust-platform` 实际运行编码代理。
- Git。
- 可选：GitHub/GitLab Token，用于访问真实仓库和同步成员/Issue。
- 可选：Azure OpenAI 或 OpenAI-compatible API Key，用于 AI Issue 生成。

## 快速启动

首次启动前安装前端依赖：

```bash
cd web-frontend
npm install
cd ..
```

启动完整开发环境：

```bash
./dev.sh
```

默认地址：

- 前端：http://localhost:5177
- 后端：http://localhost:3000
- Swagger UI：http://localhost:3000/swagger-ui

默认管理员账号：

- 用户名：`admin`
- 密码：`admin123`

`dev.sh` 会使用仓库根目录下的 `data.db` 作为本地 SQLite 数据库，并把工作空间放到 `workspaces/`。如果第一次启动后修改了 `ADMIN_INIT_PASSWORD`，已有 `admin` 用户密码不会自动重置；需要删除本地数据库或在系统中改密。

## 本地配置

`dev.sh` 会先加载仓库根目录下的 `.env.local`，不存在时再加载 `.env`。建议本地使用 `.env.local` 保存私密配置，并确保不要提交真实密钥。

最小可用配置：

```bash
JWT_SECRET=dev-secret-key-at-least-32-chars-long
ENCRYPTION_KEY=MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=
DATABASE_URL=/absolute/path/to/symphony-rust/data.db
SERVER_HOST=0.0.0.0
SERVER_PORT=3000
ADMIN_INIT_PASSWORD=admin123
SYMPHONY_BIN=/absolute/path/to/symphony-rust/target/debug/symphony-platform
SYMPHONY_WORKSPACE_ROOT=/absolute/path/to/symphony-rust/workspaces
```

### AI Issue 生成配置

AI 功能由 `web-platform` 后端读取环境变量。只有同时设置 `AZURE_OPENAI_BASEURL` 和 `AZURE_OPENAI_API_KEY` 时，AI 服务才会启用；未设置时，系统仍可正常手动创建 Issue。

Azure OpenAI 示例：

```bash
AZURE_OPENAI_BASEURL=https://your-resource.openai.azure.com
AZURE_OPENAI_API_KEY=your-azure-openai-key
AZURE_OPENAI_MODEL=your-deployment-name
AI_MODEL_FAMILY=gpt5
AI_MAX_TOKENS=4096
AI_RATE_LIMIT_PER_MINUTE=10
AI_GLOBAL_RATE_LIMIT_PER_MINUTE=30
```

OpenAI-compatible `/v1` 示例：

```bash
AZURE_OPENAI_BASEURL=https://api.openai.com/v1
AZURE_OPENAI_API_KEY=your-openai-api-key
AZURE_OPENAI_MODEL=gpt-5.5
AI_MODEL_FAMILY=gpt5
AI_MAX_TOKENS=4096
AI_RATE_LIMIT_PER_MINUTE=10
AI_GLOBAL_RATE_LIMIT_PER_MINUTE=30
```

`AI_MODEL_FAMILY` 控制请求参数兼容性：

| 值 | 适用模型 | 请求参数行为 |
|----|----------|--------------|
| `gpt5` | GPT-5 / 推理模型 / Azure 自定义部署名指向 GPT-5 时 | 使用 `max_completion_tokens`，不发送 `temperature` |
| `legacy` | 旧 Chat Completions 模型 | 使用 `max_tokens`，发送 `temperature=0.7` |

如果 `AZURE_OPENAI_MODEL` 本身以 `gpt-5`、`o1`、`o3`、`o4` 开头，未设置 `AI_MODEL_FAMILY` 时会自动推断为 `gpt5`。Azure 的部署名通常是自定义字符串，例如 `issue-generator-prod`，这时必须显式设置 `AI_MODEL_FAMILY=gpt5`，否则后端无法从部署名判断真实模型能力。

## 环境变量参考

### Web 后端

| 变量 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `JWT_SECRET` | 是 | 无 | JWT 签名密钥，长度至少 32 字符 |
| `ENCRYPTION_KEY` | 是 | 无 | Base64 编码的 32 字节 AES-GCM 密钥 |
| `DATABASE_URL` | 否 | `data.db` | SQLite 数据库路径 |
| `SERVER_HOST` | 否 | `0.0.0.0` | 后端监听地址 |
| `SERVER_PORT` | 否 | `3000` | 后端监听端口 |
| `ADMIN_INIT_PASSWORD` | 否 | 随机生成 | 首次创建 `admin` 用户时使用的密码 |
| `SYMPHONY_BIN` | 否 | `symphony-platform` | `rust-platform` 二进制路径 |
| `SYMPHONY_WORKSPACE_ROOT` | 否 | `./workspaces` | 项目服务运行工作空间根目录 |
| `MAX_CONCURRENT_CODEX` | 否 | `5` | 全局 Codex 并发上限 |
| `KANBAN_CACHE_TTL` | 否 | `10` | 看板外部 API 缓存秒数 |
| `RUST_LOG` | 否 | 代码内默认 `web_platform=info` | Rust 日志过滤规则 |

### AI 服务

| 变量 | 必填 | 默认值 | 说明 |
|------|------|--------|------|
| `AZURE_OPENAI_BASEURL` | 否 | 无 | Azure OpenAI endpoint 或 OpenAI-compatible `/v1` base URL |
| `AZURE_OPENAI_API_KEY` | 否 | 无 | AI 服务 API Key |
| `AZURE_OPENAI_MODEL` | 否 | `gpt-5.5` | Azure deployment 名或 OpenAI 模型名 |
| `AI_MODEL_FAMILY` | 否 | 按模型名推断 | `gpt5` 或 `legacy` |
| `AI_MAX_TOKENS` | 否 | `4096` | 最大输出 token 数 |
| `AI_RATE_LIMIT_PER_MINUTE` | 否 | `10` | 单用户 AI 请求限流 |
| `AI_GLOBAL_RATE_LIMIT_PER_MINUTE` | 否 | `30` | 全局 AI 请求限流 |

### 平台 Token

GitHub/GitLab Token 通常由用户在 Web 设置页或项目配置中保存。需要运行真实集成测试时，也可以使用以下变量：

| 变量 | 说明 |
|------|------|
| `GITHUB_TOKEN` | GitHub Token，需要访问测试仓库的权限 |
| `GITLAB_TOKEN` | GitLab Token，需要 `api` / `read_repository` 权限 |
| `TEST_REPO_NAME` | 测试仓库名，格式为 `owner/repo` |
| `GITLAB_BASE_URL` | GitLab 实例地址，默认 `https://gitlab.com` |

### 编排运行时 Issue API

`rust-platform` 的 `platform_api` 工具支持按平台原生 issue ID 操作 GitHub/GitLab issue：

```json
{ "action": "get_issue", "params": { "issue_id": 5 } }
```

`get_issue` 返回 issue 详情，包含 `id`、`number`、`title`、`description`、`status`、`workflow_state`、`created_at`、`updated_at`、`url`、`labels` 等字段；`status` 为 `open` 或 `closed`。

```json
{ "action": "close_issue", "params": { "issue_id": 5 } }
```

`close_issue` 会将 issue 标记为关闭并返回更新后的详情。不存在的 issue ID 会返回 `NotFound` 错误；重复关闭已关闭 issue 是幂等操作，返回的 `status` 保持为 `closed`。

## 手动启动

如果不使用 `dev.sh`，可以分开启动后端和前端。

后端：

```bash
export JWT_SECRET="dev-secret-key-at-least-32-chars-long"
export ENCRYPTION_KEY="MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="
export DATABASE_URL="$PWD/data.db"
export SERVER_PORT=3000
export ADMIN_INIT_PASSWORD=admin123
export SYMPHONY_BIN="$PWD/target/debug/symphony-platform"
export SYMPHONY_WORKSPACE_ROOT="$PWD/workspaces"

cargo run -p web-platform
```

前端：

```bash
cd web-frontend
npm install
npm run dev -- --port 5177
```

Vite 会把 `/api` 代理到 `http://localhost:3000`。

## 构建

构建 Rust workspace：

```bash
cargo build --workspace
```

构建前端：

```bash
cd web-frontend
npm run build
```

构建 release 二进制：

```bash
cargo build --release --workspace
```

## 测试

Rust 测试：

```bash
cargo test --workspace
```

仅测试 Web 后端：

```bash
cargo test -p web-platform
```

前端单元测试：

```bash
cd web-frontend
npm test
```

前端 E2E：

```bash
cd web-frontend
npm run test:e2e
```

当前仓库可能包含与不同阶段开发相关的测试夹具和草案文档。提交前应至少运行与本次改动相关的 package 测试。

## 使用 Web 平台创建项目

1. 启动 `./dev.sh`。
2. 打开 http://localhost:5177。
3. 使用 `admin` / `admin123` 登录。
4. 创建项目并填写 Git 仓库地址。
5. 在个人设置中配置 GitLab/GitHub Token。
6. 在项目页调整 workflow、Codex 命令、Hook 和并发配置。
7. 启动项目服务后，`web-platform` 会调用 `rust-platform` 二进制在独立工作空间中运行。

## `rust-platform` 独立运行

`rust-platform` 也可以不经过 Web 平台，直接使用 `WORKFLOW.md` 启动。示例：

```bash
cd rust-platform
cargo run -- /path/to/WORKFLOW.md
```

`WORKFLOW.md` 中可以通过 `$ENV_NAME` 引用环境变量，例如：

```yaml
---
tracker:
  kind: gitlab
  api_key: $GITLAB_TOKEN
  project_slug: my-group/my-repo
polling:
  interval_ms: 30000
agent:
  max_concurrent_agents: 3
  max_turns: 20
workspace:
  root: ./workspaces
codex:
  command: "codex app-server"
  turn_timeout_ms: 3600000
  stall_timeout_ms: 300000
---
你是一个编码助手，正在处理 Issue {{ issue.identifier }}: {{ issue.title }}

{{ issue.description }}
```

## 常见问题

### AI 生成提示 “AI service is not configured”

后端没有同时读取到 `AZURE_OPENAI_BASEURL` 和 `AZURE_OPENAI_API_KEY`。把它们写入 `.env.local` 后重启 `./dev.sh`。

### AI 返回 `Unsupported parameter: max_tokens`

当前部署实际是 GPT-5/推理模型，但没有使用 GPT-5 参数族。设置：

```bash
AI_MODEL_FAMILY=gpt5
```

### AI 返回 `temperature does not support 0.7`

同样是模型参数族不匹配。GPT-5/推理模型只支持默认采样参数，设置 `AI_MODEL_FAMILY=gpt5` 后后端不会发送 `temperature`。

### 登录密码不符合 `.env.local` 中的 `ADMIN_INIT_PASSWORD`

`ADMIN_INIT_PASSWORD` 只在数据库中不存在 `admin` 用户时生效。删除本地 `data.db` 后重启，或登录后修改密码。

## 许可证

Apache License 2.0。详见 [LICENSE](LICENSE)。
