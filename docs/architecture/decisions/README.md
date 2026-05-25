# 架构决策记录（ADR）索引

## 什么是 ADR

架构决策记录（Architecture Decision Record，ADR）用于记录项目中重要的架构决策，包括决策的背景、选择的方案、备选方案以及决策带来的后果。

ADR 的目的是：
- 让团队成员理解"为什么这样设计"，而不仅仅是"设计是什么"
- 为未来的维护者提供决策上下文，避免重复踩坑
- 在技术方向发生变化时，提供清晰的变更依据

## ADR 模板

```markdown
# ADR-NNN: 标题

## 状态

[proposed | accepted | deprecated | superseded by ADR-NNN]

## 上下文

描述促使这个决策产生的背景、问题和约束条件。

## 决策

描述选择的方案及其理由。

## 备选方案

列出考虑过但未采用的方案，以及未采用的原因。

## 后果

描述这个决策带来的正面和负面影响，包括需要接受的权衡。

## 日期

YYYY-MM-DD
```

## 状态说明

| 状态 | 含义 |
|------|------|
| `proposed` | 已提出，待讨论确认 |
| `accepted` | 已接受，当前有效 |
| `deprecated` | 已废弃，不再适用但未被替代 |
| `superseded` | 已被新的 ADR 替代 |

## ADR 索引

| 编号 | 标题 | 状态 | 日期 |
|------|------|------|------|
| [ADR-001](001-event-driven-orchestrator.md) | 事件驱动单线程编排器 | accepted | 2024-01-01 |
| [ADR-002](002-sqlite-over-postgres.md) | 使用 SQLite 作为 web-platform 存储 | accepted | 2024-01-01 |
| [ADR-003](003-process-manager-design.md) | 进程管理器 Lease/Generation 设计 | accepted | 2024-01-01 |
| [ADR-004](004-workflow-md-as-config.md) | WORKFLOW.md 作为单一配置源 | accepted | 2024-01-01 |
