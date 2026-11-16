# 组织规则仓库模板

把本目录复制为一个独立仓库（例如 `codex-org-config`），在 Symphony 的管理员“组织规则”页填写它的地址即可启用。设计见 `docs/方案/ai-native-sdlc-p3-design.md` 第 4、5 节。

## 目录

| 路径 | 作用 |
|---|---|
| `AGENTS.md` | 全组织的编码与文档约定，生成到每个 Codex 会话的 `CODEX_HOME/AGENTS.md` |
| `REVIEW.md` | AI 审查政策的组织层：仓库没有 `REVIEW.md` 时使用 |
| `config.toml` | 只保留模型相关配置：`model`、`model_reasoning_effort`、`model_provider`、`model_providers.<id>` 的 `name` / `base_url` / `env_key` / `wire_api`、`profiles.<id>` 的模型键。`mcp_servers`、`sandbox_*`、`approval_policy`、`shell_environment_policy` 等一律丢弃，管理页会列出被丢弃的项 |
| `skills/<name>/SKILL.md` | 组织 skill。`symphony-` 开头的名字保留给 Symphony 自带的 skill，不会生效 |
| `templates/<doc>.md` | 文档模板的组织层：`intent`、`spec`、`plan`、`intent.incident`。项目里的 `docs/changes/_templates/<doc>.md` 优先 |
| `evals/tasks/*.yaml` | `symphony-eval` 的固定任务 |
| `evals/baseline.json` | 基线报告，由 `symphony-eval --write-baseline` 生成并提交 |
| `.github/workflows/eval.yml` / `.gitlab-ci.yml` | 改动规则时运行 eval |

同步时会拒绝：符号链接、子模块、单文件超过 1 MB 或总量超过 20 MB、`auth.json` / `.env` / `*.pem` / `*.key` 等凭据文件、内容疑似包含 token 的文件。拒绝时继续使用上一次成功的版本。

## eval

```bash
symphony-eval --org-config . --baseline evals/baseline.json --out eval-report.json
```

- 每个任务在临时工作区按本仓库的模板和 `CODEX_HOME` 非交互起草一次。通过条件：起草成功、文档通过 Symphony 的章节校验、`expect` 全部满足。
- 通过率比基线低超过 5 个百分点（`--tolerance`），或基线中通过的任务失败，退出码为 1；参数或环境错误为 2。
- 第一次使用或有意调整预期后，运行 `symphony-eval --baseline evals/baseline.json --write-baseline` 并提交 `evals/baseline.json`。
- 模型输出有波动，可用 `--repeat 3` 观察稳定性；任务在所有运行中都通过才算通过。

### “通过率下降先审再合”

在仓库的分支保护里把 eval 检查设为必需：

- GitHub：Settings → Branches → 保护 `main`，勾选 “Require status checks to pass” 并选择 `eval`；需要放行时由指定审查人（CODEOWNERS）批准后由管理员合并。
- GitLab：Settings → Merge requests → “Pipelines must succeed”，并在 Approval rules 中指定审查人。

Symphony 只提供检查结果，不参与合并。

### CI 所需配置

| 变量 | 说明 |
|---|---|
| `OPENAI_API_KEY`（secret） | Codex 使用的模型凭据。用 `config.toml` 的 `model_providers.<id>.env_key` 指定其他变量名时，改为对应变量 |
| `SYMPHONY_VERSION`（可选） | 下载的 `symphony-eval` 版本，默认 `latest` |

模型凭据只放在 CI secret 中，不要写进仓库。`symphony-eval` 不访问 Symphony 服务和数据库，报告里不包含环境变量。

## 从真实 issue 整理任务

1. 选需求清楚、结果可判断的 issue：例如“导出报表需要鉴权”“结账接口 5xx”。每类文档（intent / spec / plan / intent.incident）至少一个。
2. 只保留需求本身。去掉人名、客户名、内部域名和地址、工单号、截图链接；换成中性描述（“某客户” → “企业客户”，`https://erp.corp.local` → “内部 ERP”）。
3. 上游文档（spec 需要 intent，plan 需要 intent 和 spec）放进 `context`，同样去掉敏感信息。
4. `expect.must_include` 写组织规则要求出现的内容（例如安全规则要求的“鉴权”“审计日志”），用正则；`must_not_include` 写不该出现的（例如 `TODO`、真实域名）。不要写只有某次输出才有的措辞。
5. 用 `max_total_tokens` 限制成本，避免规则改动让起草变得冗长。
6. 新任务先单独跑几次（`--tasks <dir> --repeat 3`），稳定后再加入并更新基线。
