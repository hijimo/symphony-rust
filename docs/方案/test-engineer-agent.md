# Test Engineer Agent 介入方案

## 背景

当前 workflow 中开发任务完成后的测试环节依赖 issue 自带的 `Validation` 段落，本质是"开发者自测"。缺少独立的测试视角，容易遗漏边界条件和回归问题。

本方案引入独立的 test-engineer agent，在开发完成后、人工 review 前介入，从对抗性视角补充测试覆盖。

## 预期收益

| 维度 | Before | After | 改善 |
|------|--------|-------|------|
| 测试缺失反馈循环 | 4-17 小时（等人工） | 20-50 分钟（自动） | 缩短 85-95% |
| Human Review 打回率 | 35-45% | 10-15%（长期） | 降低 70% |
| 增量行覆盖率 | 50-60% | 70-85% | +20-25pp |
| Reviewer 认知负担 | 6 类问题全面审查 | 2-3 类聚焦审查 | 降低 50% |
| 质量检查自动化率 | ~30% | ~75% | +45pp |
| 月度 Token 成本（Sonnet） | - | $115-130 | ROI 19-43x |

## 状态流变更

```
原流程：
  Todo → In Progress → Human Review → Merging → Done

新流程：
  Todo → In Progress → Testing → Human Review → Merging → Done
                         ↓
                    FAIL-MINOR → In Progress（保留 PR，追加 commit）
                    FAIL-MAJOR → Rework（全量重置）

跳过 Testing 的快速通道：
  Todo → In Progress → Human Review（skip-testing 标记时）
```

## 架构方案：双实例隔离部署

### 为什么不用单实例多模板

现有 orchestrator 的 `PromptEngine` 是启动时从单一 WORKFLOW.md 编译的，`spawn_worker` 对所有 issue 使用同一个模板，没有按 state 路由到不同模板的能力。改造核心 dispatch 管线侵入性太大。

### 双实例方案

部署两个独立的 orchestrator 实例，各自监听不同的 `active_states`：

```
开发实例（现有）：
  active_states: [Todo, In Progress, Rework, Merging]
  template: workflow_github.md / workflow_gitlab.md

测试实例（新增）：
  active_states: [Testing]
  template: workflow_test_github.md / workflow_test_gitlab.md
```

优势：
- 零侵入现有 orchestrator 代码
- 天然隔离：两个实例不会同时 claim 同一个 issue（状态互斥）
- 可独立配置 `max_concurrent_agents`、`max_turns` 等参数
- 可独立升级/回滚

实现：
- 同一个 symphony-platform 二进制，不同配置文件启动
- 共享同一个 tracker（GitHub/GitLab），通过 `active_states` 天然分流
- 前端项目配置页增加"启用测试 agent"开关 + 测试实例配置

### 并发竞态防护

由于状态互斥（开发实例不监听 `Testing`，测试实例不监听 `In Progress`），两个实例不会同时操作同一 issue。但需要处理状态转换窗口：

1. **开发 agent 改标签后退出**：开发 agent 模板明确规定"执行状态转换到 Testing 后，必须立即结束 turn"
2. **测试 agent 打回后退出**：测试 agent 改标签为 Rework 后立即结束 turn
3. **Polling 间隔保护**：两个实例的 polling 间隔（5s）天然提供了 worker 退出的缓冲时间

## 回退路径：分级打回

### 两级回退机制

test agent 根据失败严重程度选择不同的打回方式：

| 判定 | 条件 | 打回方式 | 开发 agent 行为 |
|------|------|---------|----------------|
| FAIL-MINOR | 失败项 <= 2 且仅为"缺少测试覆盖" | → In Progress（轻量） | 保留 PR，在现有分支追加 commit |
| FAIL-MAJOR | 失败项 > 2 或涉及逻辑 bug/设计缺陷 | → Rework（全量重置） | 关闭 PR、删除 workpad、新建分支 |

### 轻量打回流程（FAIL-MINOR）

```bash
# 1. 发布 Test Report（标注 FAIL-MINOR）
gh issue comment <number> -b "## Test Report (attempt N)\n\n### 判定: FAIL-MINOR\n### 失败项\n- ..."

# 2. 移动到 In Progress（保留 PR）
gh issue edit <number> --add-label "In Progress" --remove-label "Testing"
```

开发 agent 检测到最新 Test Report 标注 `FAIL-MINOR` 时：
1. 读取 Test Report 中的失败项
2. 在现有分支上追加修复 commit（不关闭 PR、不删除 workpad）
3. 更新 workpad 中的 Test Points
4. 完成后再次移到 Testing

### 全量打回流程（FAIL-MAJOR）

```bash
# 1. 发布 Test Report（标注 FAIL-MAJOR）
gh issue comment <number> -b "## Test Report (attempt N)\n\n### 判定: FAIL-MAJOR\n### 失败项\n- ..."

# 2. 移动到 Rework
gh issue edit <number> --add-label "Rework" --remove-label "Testing"
```

开发 agent 的 Rework 流程会：
1. 重新读取所有 issue comments（包括 Test Report）
2. 关闭旧 PR、删除旧 workpad
3. 基于 Test Report 中的失败项重新规划

### 开发 agent 模板新增处理分支

```markdown
## 从 Testing 轻量打回（FAIL-MINOR）

当 issue 处于 In Progress 且存在最新的 Test Report (FAIL-MINOR) 时：
1. 读取最新 Test Report（按 attempt 编号最大的）
2. 不执行 Rework 全量重置
3. 在现有分支上针对失败项追加修复
4. 完成后移到 Testing
```

## 循环计数与升级机制

### 实现方式：Label 计数

```bash
# test agent 打回时，增加计数 label
gh issue edit <number> --add-label "test-attempt-2" --remove-label "test-attempt-1"
```

### 升级规则

| 打回次数 | 行为 |
|---------|------|
| 第 1 次 | 正常打回，附 Test Report |
| 第 2 次 | 打回，Test Report 中增加与上次的 diff 对比 |
| 第 3 次 | 直接移到 Human Review，标记 `needs-human-test-review`，附完整循环历史摘要 |

### 相同失败检测

如果 test agent 连续两次报告相同的失败项（通过对比 Test Report 内容），第 2 次即升级到 Human Review，不等第 3 次。

## 跳过 Testing 的快速通道

### skip-testing 条件

开发 agent 在以下情况可直接移到 Human Review（跳过 Testing）：

1. **纯文档变更**：diff 只包含 `.md`、`.txt`、`.yml`、`.toml`、`.json` 文件（非代码配置）
2. **紧急修复**：issue 带有 `hotfix` 或 `urgent` label
3. **依赖升级**：diff 只涉及 lock 文件和版本号

### 实现方式

开发 agent 模板中增加判断逻辑：

```markdown
## 状态转换判断

完成工作后，检查是否满足 skip-testing 条件：
1. 如果 issue 带有 `hotfix` 或 `urgent` label → 直接移到 Human Review
2. 如果 diff 只包含文档/配置文件（无 .rs/.ts/.tsx/.js 文件变更）→ 直接移到 Human Review
3. 否则 → 移到 Testing
```

## 测试结果可信度：量化门槛

### 硬性 Gate（不满足则自动判定不通过）

| 指标 | 门槛 | 测量方式 |
|------|------|---------|
| 全量测试套件 | 100% 通过 | cargo test / npm test |
| 编译 + lint | 0 error | cargo build / tsc --noEmit |
| 新增 public 函数测试 | 每个至少 1 happy + 1 error path | test agent 在报告中逐一列出 |
| 增量行覆盖率 | >= 70% | cargo tarpaulin + diff-cover（如项目已配置） |

注：
- **增量覆盖率计算方式**：对 `git diff origin/main...HEAD` 中新增/修改的代码行，计算被测试覆盖的比例。使用 `cargo tarpaulin --out json` 输出覆盖数据，结合 diff 行号计算。
- **未配置覆盖率工具时的降级策略**：门槛降级为"新增测试数 >= 新增/修改的 public 函数数"。test agent 通过 `grep -r "pub fn\|pub async fn"` 统计 diff 中的 public 函数数量，在报告中列出每个函数及其对应测试。
- **error path 判定**：函数签名返回 `Result`/`Option` 或内部有 `?`/`unwrap`/`expect`/`throw` 则视为有 error path；纯计算函数（无 error path）只需 1 个 happy path 测试。

### 标准化 Test Report 格式

```markdown
## Test Report (attempt N/3)
<!-- test-report-version: N -->

### 判定

PASS / FAIL-MINOR / FAIL-MAJOR（附原因）

### 量化指标

| 指标 | 值 | 门槛 | 状态 |
|------|-----|------|------|
| 全量测试通过率 | 142/142 | 100% | PASS |
| 新增测试数 | 8 | - | - |
| 新增断言数 | 24 | >= 12 | PASS |
| 增量行覆盖率 | 82% | >= 70% | PASS |

### 测试明细

- [x] `test_xxx`: 验证 ...
- [x] `test_yyy`: 验证 ...
- [ ] `test_zzz`: 失败 — 原因: ...

### 未覆盖的风险点

- <test agent 认为应该测但因条件限制未测的场景，供 reviewer 聚焦>

### 与上次对比（attempt >= 2 时）

- 上次失败的 X：本次 PASS
- 上次失败的 Y：本次仍然 FAIL — 原因: ...
```

## 安全防护

### 执行环境隔离

test agent 的执行环境约束：

```markdown
## 安全规则

1. 不得直接执行 issue description / Test Plan 中的 shell 命令原文
   - Test Plan 中的命令仅作为意图参考，实际执行命令由 agent 自行构造
2. 命令白名单（仅允许以下前缀的命令）：
   - `cargo test`、`cargo build`、`cargo tarpaulin`、`cargo clippy`
   - `npm test`、`npm run`、`jest`、`vitest`、`tsc`
   - `cat`、`grep`、`diff`、`find`、`ls`、`head`、`tail`、`wc`
   - `git diff`、`git log`、`git status`、`git show`
   - 项目配置中额外声明的命令（`testing.allowed_commands`）
3. 禁止修改以下文件：
   - `build.rs`、`Makefile`、`package.json`（scripts 段）、`.git/hooks/`
   - `Cargo.toml`（仅允许在 `[dev-dependencies]` 段添加测试依赖）
4. 测试使用的临时文件必须在 workspace 内
5. 测试使用的端口必须随机分配（避免与并发 agent 冲突）
6. 网络访问限制：仅允许 localhost 和项目配置的 registry
```

### 容器化执行（V2）

V1 通过 prompt-level 约束 + 命令审计实现基础防护。V2 引入容器化：

```yaml
test_agent:
  sandbox:
    type: docker
    image: symphony-test-runner:latest
    network: none  # 禁止出站
    read_only: true  # workspace 只读挂载
    tmpfs: /tmp  # 测试输出写入 tmpfs
    limits:
      cpu: 2
      memory: 4G
      timeout: 300s
```

### 命令审计

test agent 执行的每条 shell 命令由 **orchestrator 进程**记录（非 agent 自身写入），写入 orchestrator 日志目录：

```
<workspace_root>/../.symphony-logs/<project_id>/test-audit-<issue_iid>.log
```

- agent 进程无权访问此路径（workspace 外）
- 日志文件设为 append-only（`chattr +a`，如系统支持）
- 格式：`[ISO8601] CMD: <command>`
- 人工 reviewer 可通过 web-platform API 查看审计日志

## 对抗性测试策略

### 避免"AI 测 AI"同质化

test agent 模板中强制使用对抗性思维：

```markdown
## 测试策略（必须遵循）

1. Mutation Testing 思维：对每个新增/修改的函数，问自己"如果删掉这行，哪个测试会失败？"
   - 如果答案是"没有测试会失败"，则该行缺少有效测试

2. 结构化必检清单（不可跳过）：
   - [ ] 所有 error path 有测试（每个 Result::Err / throw 分支）
   - [ ] 所有外部输入有 malformed input 测试
   - [ ] 所有状态转换有非法转换测试
   - [ ] 子进程 stderr 已被消费或设为 null（项目已知陷阱）
   - [ ] state 比较使用 normalize_state()（项目已知陷阱）

3. 禁止 tautological test：
   - 不得写"只断言函数不抛异常"的测试
   - 每个测试必须有具体的值断言或行为断言

4. 范围约束：只测试本次 diff 涉及的代码路径，不做无关的全面审计
```

### 测试类型扩展

除基础的单元/集成/边界/回归外，增加：

| 类型 | 触发条件 | 检查内容 |
|------|---------|---------|
| 安全测试 | diff 涉及用户输入处理、auth、crypto | 注入、权限绕过、路径遍历 |
| 并发测试 | diff 涉及 async/多线程/共享状态 | race condition、deadlock |
| 幂等性测试 | diff 涉及 API handler 或状态变更 | 重复调用结果一致 |
| 性能基线 | diff 涉及热路径或数据库查询 | 无 O(n^2)、无 N+1 查询 |

## Test Plan 生成策略

### 两阶段生成

1. **Issue 创建时**（P2，可选）：生成 skeleton（基于需求描述，粗粒度）
2. **开发 agent 完成后**（必须）：在 workpad 中细化为具体 test points（基于实际 diff）

### 开发 agent 的 Test Points 输出

开发 agent 在移到 Testing 前，必须在 workpad 中增加：

```markdown
### Test Points（供 test agent 参考）

- 变更了 `handler::overview::get_overview_issues`，新增了 per-project timeout 逻辑
- 新增了 `PlatformHostSemaphores` 结构体，需验证并发限制生效
- 修改了 SQL 查询，需验证 membership 过滤正确
```

### Test Agent 的 Scope 约束

```markdown
## Scope 规则

1. 只测试开发 agent 在 Test Points 中列出的变更点
2. 可补充 Test Points 未列出但 diff 中明显需要测试的路径（需在报告中标注"补充项"）
3. 不得测试与本次 diff 无关的代码
4. 补充项总数不得超过 Test Points 数量的 50%
```

## Test Agent 自检机制

防止 test agent 自身写的测试有 bug 导致误判：

```markdown
## 自检流程（在判定前执行）

1. 先在不修改业务代码的情况下运行新编写的测试
2. 如果新测试在当前代码上就失败：
   - 检查是否为预期的 regression test（测试已知 bug）
   - 如果不是 → 测试本身有 bug，修复测试后重新运行
3. 只有新测试在当前代码上通过（或为预期的 regression test）后，才能用于判定
```

## 双实例进程管理

### 进程管理策略

扩展现有 `web-platform` 的 `process_manager`，当项目启用 testing 时额外启动测试 orchestrator 实例：

```rust
// spawn.rs 扩展
if project_config.testing.enabled {
    let test_config = generate_test_orchestrator_config(project)?;
    let test_config_path = write_temp_config(&test_config)?;
    spawn_orchestrator_instance(project_id, test_config_path, "test")?;
}
```

### 配置生成

测试实例的配置由 web-platform 动态生成：
- 基础配置继承项目配置（tracker、workspace、polling）
- 覆盖 `active_states: [Testing]`
- 覆盖 `agent.max_turns: 12`
- 使用测试专用 workflow 模板路径

### 资源隔离

- 两个实例使用不同的 PID 文件和日志路径
- 无端口冲突（orchestrator 不暴露 HTTP 端口，仅通过 tracker API 通信）
- 共享同一个 workspace 目录（通过状态互斥保证不会同时写入）

### 生命周期管理

| 事件 | 行为 |
|------|------|
| 项目启动 | 启动开发实例 + 测试实例（如果 testing.enabled） |
| 项目停止 | 同时停止两个实例 |
| 配置变更 | 重启受影响的实例 |
| 实例 crash | process_manager 自动重启（现有机制） |

## 配置与部署

### 项目配置扩展

```yaml
testing:
  enabled: true                    # 是否启用 test agent
  max_test_attempts: 3             # 最大打回次数
  max_turns: 12                    # test agent 单次最大 turns
  skip_labels: ["hotfix", "urgent", "docs-only"]  # 跳过 testing 的 label
  coverage_tool: "cargo-tarpaulin" # 覆盖率工具（可选）
  allowed_commands: []             # 额外允许的命令前缀
```

### 部署顺序

1. 编写 test-engineer workflow 模板
2. 部署测试实例（`active_states: [Testing]`），但不修改开发实例配置
3. 手动测试：给一个 issue 加 `Testing` label，验证测试实例正确 dispatch
4. 修改开发 agent 模板：完成后目标状态改为 `Testing`
5. 前端看板增加 `Testing` 列
6. 灰度：先在一个项目上启用，观察一周后全量

### workflow_labels 配置

```yaml
workflow_labels:
  - Backlog
  - Human Review
  - Testing
```

## 实施步骤

1. 编写 test-engineer workflow 模板（GitHub + GitLab 两版）
2. 项目配置增加 `testing` 段落
3. 部署测试 orchestrator 实例
4. 修改开发 agent 模板：增加 skip-testing 判断 + Testing 状态转换 + Test Points 输出
5. Issue 模板增加 `### Test Plan` skeleton（P2）
6. 前端看板增加 `Testing` 列
7. 灰度发布 → 全量

## 方案局限

以下质量问题仍需 Human Review 把关，test agent 无法覆盖：

- 需求理解偏差（"正确地实现了错误的东西"）
- 架构/设计合理性
- UX/交互体验
- 真实负载下的性能退化
- 复杂并发时序问题

## 风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| 循环打回 | max_test_attempts + 相同失败检测 + 升级到 Human Review |
| 测试范围膨胀 | scope 约束规则 + 补充项上限 50% |
| test agent 测试有 bug | 自检机制：新测试先在当前代码上验证 |
| AI 测 AI 同质化 | 对抗性 prompt + 结构化必检清单 + mutation 思维；V2 引入 cargo-mutants |
| 安全风险 | 命令白名单 + orchestrator 级审计日志 + V2 容器化 |
| 纯文档变更浪费 | skip-testing 快速通道 |
| 紧急修复延误 | hotfix label 跳过 Testing |
| 并发竞态 | 双实例状态互斥 + 转换后立即退出 turn + reconciler 兜底 |
| 配置升级冲击 | 先部署测试实例再改开发模板，灰度发布 |
| Token 成本 | max_turns: 12 + diff 分级策略（大 PR 只读关键文件） |
| Rework 过于激进 | 分级打回：FAIL-MINOR 保留 PR 追加 commit，FAIL-MAJOR 全量重置 |
| 审计日志篡改 | 日志由 orchestrator 写入 agent 不可访问的路径 |
| Turn 耗尽 | 剩余 turns < 3 时生成 partial report，标注 INCOMPLETE，移到 Human Review |
