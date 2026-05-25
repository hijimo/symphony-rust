# web-frontend 架构文档

## 1. 概述

web-frontend 是 Symphony 的管理控制台，基于 React 18.3 + TypeScript 5.5 构建的单页应用（SPA），提供项目管理、Issue 管理、看板视图、告警配置、并发控制等可视化管理能力。

## 2. 技术选型

| 类别 | 技术 | 版本 | 选型理由 |
|------|------|------|----------|
| UI 框架 | React | 18.3 | 生态成熟，Concurrent Mode |
| 语言 | TypeScript | 5.5 | 类型安全，IDE 支持好 |
| UI 组件库 | MUI (Material UI) | v6 | 完整组件体系，与设计系统契合 |
| CSS 框架 | Tailwind CSS | 3.4 | 原子化 CSS，快速布局 |
| 构建工具 | Vite | 6.0 | 极速 HMR，ESM 原生支持 |
| 状态管理 | Zustand | 5.0 | 轻量，无样板代码，支持持久化 |
| 路由 | React Router | 6.26 | 声明式路由，嵌套路由支持 |
| HTTP 客户端 | Axios | 1.7 | 拦截器机制，请求/响应转换 |
| 单元测试 | Vitest + React Testing Library | 2.x / 16.x | Vite 原生集成，API 与 Jest 兼容 |
| E2E 测试 | Playwright | 1.49 | 跨浏览器，可靠的自动化测试 |
| Mock | MSW (Mock Service Worker) | 2.x | 拦截网络请求，测试与开发共用 |

## 3. 目录结构

```
web-frontend/src/
├── main.tsx              # 应用入口，挂载 React 根节点
├── App.tsx               # 根组件，路由配置
├── router.tsx            # React Router 路由定义
├── theme.ts              # MUI 主题配置（设计系统）
├── index.css             # 全局样式，Tailwind 指令
├── vite-env.d.ts         # Vite 环境变量类型声明
│
├── pages/                # 页面组件（路由级别）
│   ├── Login.tsx
│   ├── Settings.tsx
│   ├── AdminUsers.tsx
│   ├── AdminConcurrency.tsx
│   ├── AdminAlerts.tsx
│   ├── AdminConfig.tsx
│   └── projects/
│       ├── ProjectListPage.tsx
│       ├── CreateProjectPage.tsx
│       ├── ProjectSettingsPage.tsx
│       ├── ProjectMembersPage.tsx
│       ├── KanbanPage.tsx
│       ├── IssueDetailPage.tsx
│       ├── CreateIssuePage.tsx
│       └── MrDetailPage.tsx
│
├── components/           # 可复用组件
│
├── api/                  # API 调用层
│   ├── client.ts         # Axios 实例配置
│   ├── caseTransform.ts  # camelCase ↔ snake_case 转换
│   ├── auth.ts
│   ├── projects.ts
│   ├── issues.ts
│   ├── kanban.ts
│   ├── members.ts
│   ├── alerts.ts
│   ├── concurrency.ts
│   ├── workflow.ts
│   ├── aiGenerate.ts
│   ├── admin.ts
│   ├── adminConfig.ts
│   ├── adminNetworkProxy.ts
│   ├── user.ts
│   └── types.ts
│
├── store/                # Zustand 状态管理
│   ├── auth.ts           # 认证状态（localStorage 持久化）
│   ├── projectStore.ts
│   ├── issueStore.ts     # 含 AI 流式生成状态
│   ├── kanbanStore.ts
│   ├── alertStore.ts
│   └── concurrencyStore.ts
│
├── types/                # TypeScript 类型定义
│
└── test/                 # 测试工具与 Mock 配置
```

## 4. 路由设计

```
/login                          # 登录页
/projects                       # 项目列表
/projects/new                   # 创建项目
/projects/:id/settings          # 项目设置（配置、WORKFLOW.md）
/projects/:id/members           # 项目成员管理
/projects/:id/kanban            # 看板视图
/projects/:id/issues/create     # 创建 Issue
/projects/:id/issues/:iid       # Issue 详情
/projects/:id/mrs/:iid          # MR 详情
/admin/users                    # 用户管理（admin）
/admin/concurrency              # 并发控制（admin）
/admin/alerts                   # 告警配置（admin）
/admin/config                   # 系统配置（admin）
/settings                       # 个人设置
```

路由守卫：未登录用户访问受保护路由时自动重定向到 `/login`；非 admin 用户访问 `/admin/*` 时返回 403。

## 5. 状态管理

使用 Zustand 5.0 管理全局状态，各 Store 职责单一，按需订阅。

### auth store（`store/auth.ts`）

- 存储 `token`、`user`（id、role、username）
- 使用 `localStorage` 持久化，页面刷新后自动恢复登录状态
- 提供 `login()`、`logout()` action
- `logout()` 清除 localStorage 并重定向到 `/login`

### projectStore（`store/projectStore.ts`）

- 存储项目列表、当前项目详情
- 管理服务运行状态（running / stopped / starting）
- 提供项目 CRUD action

### issueStore（`store/issueStore.ts`）

- 存储 Issue 列表、当前 Issue 详情
- 管理 AI 流式生成状态：`generating`（布尔）、`streamContent`（累积文本）
- AI 生成通过 SSE 逐 token 更新 `streamContent`，组件实时渲染

### kanbanStore（`store/kanbanStore.ts`）

- 存储看板列（状态列表）和各列 Issue
- 管理拖拽状态和乐观更新

### alertStore（`store/alertStore.ts`）

- 存储告警规则列表
- 管理告警启用/禁用状态

### concurrencyStore（`store/concurrencyStore.ts`）

- 存储并发控制配置
- 管理全局并发上限和按状态并发上限

## 6. API 层设计

### Axios 客户端（`api/client.ts`）

```
Axios 实例
├── baseURL: /api（由 Vite proxy 转发到 :3000）
├── 请求拦截器
│   ├── 注入 Authorization: Bearer <token>（从 auth store 读取）
│   └── 请求体 camelCase → snake_case 转换
└── 响应拦截器
    ├── 响应体 snake_case → camelCase 转换
    └── 401 检测 → 自动调用 logout()，重定向到 /login
```

### camelCase ↔ snake_case 转换（`api/caseTransform.ts`）

前端使用 camelCase 命名约定，后端 API 使用 snake_case。`caseTransform.ts` 在 Axios 拦截器中自动完成双向转换，业务代码无需手动处理。

### AI 生成流式接口（`api/aiGenerate.ts`）

- 使用 `fetch` API（非 Axios）建立 SSE 连接
- 逐行解析 `data:` 事件，追加到 `issueStore.streamContent`
- 连接关闭或出错时更新 `generating` 状态

## 7. 设计系统

遵循 `design/design.md` 中定义的 Architectural Digital Desktop 风格，通过 `theme.ts` 统一配置 MUI 主题。

### 色彩体系（Tonal Layering）

| 层级 | 用途 | 色值 |
|------|------|------|
| Primary | 主操作按钮、强调色 | `#003ea8` |
| Surface 0 | 页面背景 | 最深层 |
| Surface 1 | 侧边栏、导航 | 次深层 |
| Surface 2 | 卡片、面板 | 中间层 |
| Surface 3 | 输入框、悬浮元素 | 最浅层 |

### 排版规范

- 字体：Inter（Google Fonts）
- Type Scale 遵循 MUI 默认 scale，标题使用 `fontWeight: 600`

### 间距与布局

- Base unit：4px（MUI spacing(1) = 4px）
- 侧边栏宽度：256px
- 主内容区：12 列网格

### 组件样式规范

| 组件 | 样式规范 |
|------|----------|
| 按钮 | 渐变背景（primary），圆角 4px，无阴影 |
| 输入框 | Filled variant，圆角 4px |
| 卡片 | 无阴影静态卡片，圆角 8px |
| 容器/面板 | 圆角 8px |

## 8. 测试策略

### 单元测试（Vitest + React Testing Library）

- 测试文件位于 `__tests__/` 目录或与组件同目录的 `*.test.tsx`
- 使用 RTL 的 `render`、`screen`、`userEvent` 测试组件行为
- Store 测试直接调用 action，验证状态变更
- 运行命令：`npm run test`（watch 模式）、`npm run test:run`（单次）

### E2E 测试（Playwright）

- 测试文件位于项目根目录的 `playwright/` 或 `e2e/` 目录
- 覆盖关键用户流程：登录、创建项目、启动服务、查看看板
- 运行命令：`npm run test:e2e`

### Mock（MSW）

- `test/` 目录包含 MSW handler 配置
- 拦截 `/api/*` 请求，返回预设响应
- 单元测试和开发模式均可使用，保证测试与真实 API 行为一致
