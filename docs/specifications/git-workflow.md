# Git 工作流规范

## 分支策略

| 分支类型 | 命名格式 | 说明 |
|----------|----------|------|
| 主分支 | `main` | 始终保持可发布状态，受保护，不直接推送 |
| 功能分支 | `feature/xxx` 或 `{user}/xxx` | 新功能开发，从 `main` 创建 |
| 修复分支 | `fix/xxx` 或 `{user}/fix/xxx` | Bug 修复，从 `main` 创建 |
| 文档分支 | `docs/xxx` | 文档更新 |
| 重构分支 | `refactor/xxx` | 代码重构，不改变功能 |

分支名称使用小写字母、数字和连字符，简洁描述变更内容。支持用户名前缀格式，例如：
- `feature/mr-idempotency`
- `fix/state-normalization-reconciler`
- `hijimo/issue-15-kanban-redirect`
- `hijimo/codex/issue-4-default-todo`

---

## Commit Message 格式

遵循 [Conventional Commits](https://www.conventionalcommits.org/) 规范：

```
type(scope): description

[可选 body]

[可选 footer]
```

### type 枚举

| type | 说明 |
|------|------|
| `feat` | 新功能 |
| `fix` | Bug 修复 |
| `docs` | 文档变更 |
| `chore` | 构建、依赖、配置等杂项 |
| `refactor` | 代码重构（不改变功能） |
| `test` | 测试相关 |
| `perf` | 性能优化 |
| `style` | 代码格式（不影响逻辑） |

### scope 示例

`web-platform`、`rust-platform`、`codex`、`db`、`auth`、`projects`、`alerts`、`proxy`

### 示例

```
feat(web-platform): add idempotent merge request creation

Implements Idempotency-Key header support for POST /api/projects/:id/mrs.
Stores request/response in idempotency_requests table to deduplicate
concurrent MR creation requests.

Closes #15
```

```
fix(rust-platform): normalize state strings with space-to-underscore

Reconciler was incorrectly terminating running workers because
normalize_state() didn't convert spaces to underscores, causing
mismatch between tracker state_key and active_states config.
```

```
chore: address clippy warnings
```

---

## PR 流程

### 完整流程

```
1. 从 main 创建功能分支
   git checkout -b feature/my-feature

2. 开发并提交（遵循 commit message 规范）
   git add <specific-files>
   git commit -m "feat(scope): description"

3. 推送到远程
   git push -u origin feature/my-feature

4. 创建 Pull Request
   - 标题简洁（< 70 字符），遵循 commit message 格式
   - 描述说明：变更内容、测试方式、相关 Issue

5. CI 检查通过

6. Code Review（至少 1 人 approve）

7. Squash Merge 到 main（保持 main 历史整洁）

8. 删除功能分支
```

### PR 描述模板

```markdown
## 变更内容

简要描述本 PR 做了什么。

## 测试方式

- [ ] 单元测试：`cargo test`
- [ ] 集成测试：`cargo test --test <test_name>`
- [ ] 手动测试步骤（如有）

## 关联 Issue

Closes #<issue_number>
```

---

## CI 检查

所有 PR 合并前必须通过以下 CI 检查：

### Rust 检查

```bash
# 格式检查
cargo fmt --check

# Lint（所有 warning 视为 error）
cargo clippy -- -D warnings

# 测试
cargo test
```

### 前端检查

```bash
# 依赖安装
npm install

# Lint
npm run lint

# 测试
npm test

# 构建验证
npm run build
```

### 检查失败处理

- `cargo fmt --check` 失败：本地执行 `cargo fmt` 后重新提交
- `cargo clippy` 失败：修复所有 clippy 警告，不允许用 `#[allow(...)]` 绕过（除非有充分理由并在 PR 中说明）
- `cargo test` 失败：修复失败的测试，不允许跳过或删除测试

---

## 代码审查要求

### 审查重点

- **正确性**：逻辑是否正确，边界条件是否处理
- **安全性**：是否有 SQL 注入、权限绕过、敏感信息泄露等风险
- **已知陷阱**：是否遵守编码规范中的已知陷阱（state 归一化、stderr drain、错误消息等）
- **测试覆盖**：新功能是否有对应测试，测试是否覆盖关键路径
- **向后兼容**：数据库迁移是否遵循只增不删原则

### 审查规范

- 审查意见分级：`nit`（可选改进）、`suggestion`（建议）、`blocker`（必须修改）
- `blocker` 级别意见必须解决后才能合并
- 审查者应在 24 小时内完成审查
- 作者应在收到审查意见后 48 小时内响应

### 不需要 Review 的情况

- 文档拼写/格式修正
- 依赖版本更新（需 CI 通过）
- 自动生成的代码变更
