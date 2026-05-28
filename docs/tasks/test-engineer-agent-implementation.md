# Test Engineer Agent 实现

## 概述

实现双实例隔离部署的 test-engineer agent。同一个 symphony 二进制通过不同配置文件启动两个 orchestrator 实例：
- **开发实例**：监听 `[Todo, In Progress, Rework, Merging]` 状态
- **测试实例**：监听 `[Testing]` 状态

零侵入 orchestrator 核心代码，通过 WORKFLOW_TEST.md 配置文件实现行为差异。

## 变更范围

### Phase 1: 数据库迁移 + 项目模型

- `web-platform/migrations/V010__testing_config.sql` — 新增 9 个 testing 相关列
- `web-platform/src/models/project.rs` — Project/ProjectUpdate/UpdateProjectRequest 扩展
- `web-platform/src/repository/sqlite.rs` — row mapping、update_project、admin query 适配
- `web-platform/src/handlers/projects.rs` — 验证逻辑 (max_attempts 1-5, max_turns 5-30)

### Phase 2: 测试 Agent 工作流模板

- `web-platform/src/templates/workflow_test_github.md` — GitHub 测试 Agent 模板
- `web-platform/src/templates/workflow_test_gitlab.md` — GitLab 测试 Agent 模板
- `web-platform/src/templates/mod.rs` — render_test_template()、WorkflowTemplateContext 扩展

模板包含：对抗性测试策略、Test Report 格式 (PASS/FAIL-MINOR/FAIL-MAJOR)、作用域约束、自检机制、安全规则（命令白名单）、循环检测、turn 预算管理。

### Phase 3: 双实例进程管理

- `web-platform/src/process_manager/mod.rs` — test_processes/test_locks DashMap + helper methods
- `web-platform/src/process_manager/spawn.rs` — spawn_test_symphony() 函数
- `web-platform/src/process_manager/watcher.rs` — spawn_test_watcher() + watch_test_process()
- `web-platform/src/handlers/project_service.rs` — start/stop/restart 双实例联动
- `web-platform/src/repository/traits.rs` — update_testing_service_status trait method
- `web-platform/src/repository/sqlite.rs` — update_testing_service_status 实现

### Phase 4: 开发 Agent 模板修改

- `web-platform/src/templates/workflow_github.md` — Testing Gate 逻辑、FAIL-MINOR 处理
- `web-platform/src/templates/workflow_gitlab.md` — 同上
- `web-platform/src/handlers/project_workflow.rs` — WorkflowTemplateContext 字段补全

### Phase 5: 前端看板 Testing 列

- `web-frontend/src/types/kanban.ts` — KanbanTestingColumn 类型
- `web-frontend/src/types/overview.ts` — ProjectIssuesEntry.testing 字段
- `web-frontend/src/store/kanbanStore.ts` — assembleKanbanData 传递 testing
- `web-frontend/src/components/kanban/KanbanBoard.tsx` — 条件渲染第 4 列
- `web-frontend/src/components/overview/ProjectKanbanSection.tsx` — 同上
- `web-platform/src/models/kanban.rs` — TestingColumn struct
- `web-platform/src/models/overview.rs` — ProjectIssuesEntry.testing 字段
- `web-platform/src/handlers/kanban.rs` — 条件查询 Testing label issues
- `web-platform/src/handlers/overview.rs` — testing: None 占位

### Phase 6: 前端项目设置

- `web-frontend/src/types/index.ts` — Project/UpdateProjectParams/ServiceStatusData 扩展
- `web-frontend/src/pages/projects/ProjectSettingsPage.tsx` — 测试 Agent 配置 UI

## 关键设计决策

1. **Optional testing column** — `testing?: KanbanTestingColumn` 使未启用测试的项目不显示该列
2. **独立 WORKFLOW_TEST.md** — 测试实例读取自己的文件，与开发实例无冲突
3. **并行 DashMap** — `test_processes` 与 `processes` 并列，不改变现有 API 表面
4. **条件看板查询** — 仅当 `testing_enabled = true` 时查询 Testing issues
5. **动态 Grid** — 根据数据存在性决定 3 列或 4 列布局
6. **分级回滚** — FAIL-MINOR 保留 PR 追加 commit，FAIL-MAJOR 完整 Rework 重置
7. **Testing Gate** — hotfix/urgent 标签或 docs-only 变更跳过测试

## 验证

- `cargo build && cargo test` — 24 tests passed (web-platform)
- `npm run build && npx vitest run` — 164 tests passed (web-frontend)
