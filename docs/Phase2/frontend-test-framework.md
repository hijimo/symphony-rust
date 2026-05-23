# Phase 2 前端测试自动化框架设计

## 1. 测试目录结构

```
web-frontend/
├── vitest.config.ts                          # 单元/集成测试配置
├── playwright.config.ts                      # E2E 测试配置
├── src/
│   ├── test/
│   │   ├── setup.ts                          # Vitest 全局 setup（已有）
│   │   ├── utils.tsx                         # 自定义 render（扩展）
│   │   ├── fixtures/
│   │   │   ├── projects.ts                   # 项目 mock 数据
│   │   │   ├── members.ts                    # 成员 mock 数据
│   │   │   └── service-status.ts             # 服务状态 mock 数据
│   │   └── mocks/
│   │       ├── handlers.ts                   # Phase 1 handlers（已有）
│   │       ├── handlers-projects.ts          # 项目 API handlers
│   │       ├── handlers-members.ts           # 成员 API handlers
│   │       ├── handlers-workflow.ts          # Workflow API handlers
│   │       ├── handlers-service.ts           # 服务控制 API handlers
│   │       └── server.ts                     # MSW server（扩展）
│   ├── api/__tests__/
│   │   └── projects.test.ts                  # 项目 API 层测试
│   ├── store/__tests__/
│   │   └── projectStore.test.ts              # 项目 Store 测试
│   ├── components/projects/__tests__/
│   │   ├── ProjectCard.test.tsx              # 项目卡片组件测试
│   │   ├── GitUrlInput.test.tsx              # Git URL 输入组件测试
│   │   ├── ServiceStatusBadge.test.tsx       # 服务状态徽章测试
│   │   ├── ServiceControlPanel.test.tsx      # 服务控制面板测试
│   │   ├── MemberTable.test.tsx              # 成员表格测试
│   │   └── WorkflowEditor.test.tsx           # 工作流编辑器测试
│   └── pages/projects/__tests__/
│       ├── ProjectListPage.test.tsx          # 项目列表页测试
│       ├── CreateProjectPage.test.tsx        # 创建项目页测试
│       ├── ProjectSettingsPage.test.tsx      # 项目设置页测试
│       └── ProjectMembersPage.test.tsx       # 成员管理页测试
├── e2e/
│   ├── fixtures/
│   │   └── test-data.ts                      # E2E 测试数据
│   ├── page-objects/
│   │   ├── BasePage.ts                       # 基础页面对象
│   │   ├── LoginPage.ts                      # 登录页
│   │   ├── ProjectListPage.ts               # 项目列表页
│   │   ├── CreateProjectPage.ts             # 创建项目页
│   │   ├── ProjectSettingsPage.ts           # 项目设置页
│   │   └── ProjectMembersPage.ts            # 成员管理页
│   ├── project-crud.spec.ts                  # 项目 CRUD 流程
│   ├── service-management.spec.ts            # 服务管理流程
│   ├── member-management.spec.ts             # 成员管理流程
│   └── permission.spec.ts                    # 权限验证流程
└── .gitlab-ci.yml                            # CI 配置（前端部分）
```

---

## 2. 测试基础设施代码

### 2.1 Test Fixtures

#### `src/test/fixtures/projects.ts`

```typescript
import type { Project, ServiceStatus } from '../../types';

export type MockProject = {
  id: number;
  name: string;
  description: string;
  git_url: string;
  platform: 'gitlab' | 'github';
  platform_host: string;
  namespace: string;
  repo_name: string;
  default_branch: string;
  workflow_template: 'default' | 'custom';
  service_status: ServiceStatus;
  service_pid: number | null;
  max_concurrent_agents: number;
  auto_restart: boolean;
  member_count: number;
  my_role: 'owner' | 'member' | null;
  created_by: number;
  created_at: string;
  updated_at: string;
};

export const mockProjects: MockProject[] = [
  {
    id: 1,
    name: 'symphony-core',
    description: 'Symphony 核心引擎',
    git_url: 'https://gitlab.com/team/symphony-core',
    platform: 'gitlab',
    platform_host: 'gitlab.com',
    namespace: 'team',
    repo_name: 'symphony-core',
    default_branch: 'main',
    workflow_template: 'default',
    service_status: 'running',
    service_pid: 12345,
    max_concurrent_agents: 2,
    auto_restart: true,
    member_count: 3,
    my_role: 'owner',
    created_by: 1,
    created_at: '2024-06-01T10:00:00Z',
    updated_at: '2024-06-15T14:30:00Z',
  },
  {
    id: 2,
    name: 'web-frontend',
    description: '前端管理平台',
    git_url: 'https://github.com/org/web-frontend',
    platform: 'github',
    platform_host: 'github.com',
    namespace: 'org',
    repo_name: 'web-frontend',
    default_branch: 'main',
    workflow_template: 'custom',
    service_status: 'stopped',
    service_pid: null,
    max_concurrent_agents: 1,
    auto_restart: false,
    member_count: 2,
    my_role: 'member',
    created_by: 2,
    created_at: '2024-06-10T08:00:00Z',
    updated_at: '2024-06-10T08:00:00Z',
  },
  {
    id: 3,
    name: 'data-pipeline',
    description: '数据处理管道',
    git_url: 'https://gitlab.example.com/infra/data-pipeline',
    platform: 'gitlab',
    platform_host: 'gitlab.example.com',
    namespace: 'infra',
    repo_name: 'data-pipeline',
    default_branch: 'develop',
    workflow_template: 'default',
    service_status: 'error',
    service_pid: null,
    max_concurrent_agents: 4,
    auto_restart: true,
    member_count: 5,
    my_role: 'owner',
    created_by: 1,
    created_at: '2024-05-20T12:00:00Z',
    updated_at: '2024-06-14T09:00:00Z',
  },
];

export const mockProjectStopped = mockProjects[1];
export const mockProjectRunning = mockProjects[0];
export const mockProjectError = mockProjects[2];

export function createMockProject(overrides: Partial<MockProject> = {}): MockProject {
  return {
    id: 99,
    name: 'test-project',
    description: 'Test project',
    git_url: 'https://gitlab.com/test/project',
    platform: 'gitlab',
    platform_host: 'gitlab.com',
    namespace: 'test',
    repo_name: 'project',
    default_branch: 'main',
    workflow_template: 'default',
    service_status: 'stopped',
    service_pid: null,
    max_concurrent_agents: 2,
    auto_restart: true,
    member_count: 1,
    my_role: 'owner',
    created_by: 1,
    created_at: '2024-06-01T00:00:00Z',
    updated_at: '2024-06-01T00:00:00Z',
    ...overrides,
  };
}
```

#### `src/test/fixtures/members.ts`

```typescript
export type MockProjectMember = {
  user_id: number;
  username: string;
  display_name: string;
  role: 'owner' | 'member';
  synced_from: 'gitlab' | 'github' | null;
  created_at: string;
};

export const mockMembers: MockProjectMember[] = [
  {
    user_id: 1,
    username: 'admin',
    display_name: 'Administrator',
    role: 'owner',
    synced_from: null,
    created_at: '2024-06-01T10:00:00Z',
  },
  {
    user_id: 2,
    username: 'john',
    display_name: 'John Doe',
    role: 'member',
    synced_from: 'gitlab',
    created_at: '2024-06-05T08:00:00Z',
  },
  {
    user_id: 3,
    username: 'jane',
    display_name: 'Jane Smith',
    role: 'member',
    synced_from: null,
    created_at: '2024-06-10T12:00:00Z',
  },
];

export const mockSyncResult = {
  added: 2,
  skipped: 1,
  unmatched: ['external_user1', 'external_user2'],
};
```

#### `src/test/fixtures/service-status.ts`

```typescript
export type MockServiceStatus = {
  status: 'running' | 'stopped' | 'starting' | 'stopping' | 'error' | 'failed';
  pid: number | null;
  started_at: string | null;
  uptime_seconds: number | null;
  restart_count: number;
  error_message: string | null;
};

export const serviceStatusRunning: MockServiceStatus = {
  status: 'running',
  pid: 12345,
  started_at: '2024-06-15T10:00:00Z',
  uptime_seconds: 3600,
  restart_count: 0,
  error_message: null,
};

export const serviceStatusStopped: MockServiceStatus = {
  status: 'stopped',
  pid: null,
  started_at: null,
  uptime_seconds: null,
  restart_count: 0,
  error_message: null,
};

export const serviceStatusError: MockServiceStatus = {
  status: 'error',
  pid: null,
  started_at: '2024-06-15T09:00:00Z',
  uptime_seconds: null,
  restart_count: 3,
  error_message: 'Process exited with code 1',
};

export const serviceStatusStarting: MockServiceStatus = {
  status: 'starting',
  pid: null,
  started_at: null,
  uptime_seconds: null,
  restart_count: 0,
  error_message: null,
};

export const serviceStatusFailed: MockServiceStatus = {
  status: 'failed',
  pid: null,
  started_at: null,
  uptime_seconds: null,
  restart_count: 3,
  error_message: 'Max restart attempts exceeded',
};
```

### 2.2 MSW Handlers

#### `src/test/mocks/handlers-projects.ts`

```typescript
import { http, HttpResponse } from 'msw';
import { mockProjects, createMockProject } from '../fixtures/projects';

const BASE_URL = '*/api';

export const projectHandlers = [
  // GET /api/projects - 项目列表
  http.get(`${BASE_URL}/projects`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const url = new URL(request.url);
    const search = url.searchParams.get('search') || '';
    const platform = url.searchParams.get('platform');
    const status = url.searchParams.get('status');
    const pageNo = parseInt(url.searchParams.get('pageNo') || '1');
    const pageSize = parseInt(url.searchParams.get('pageSize') || '20');

    let filtered = [...mockProjects];
    if (search) {
      filtered = filtered.filter(
        (p) => p.name.includes(search) || p.description.includes(search),
      );
    }
    if (platform) {
      filtered = filtered.filter((p) => p.platform === platform);
    }
    if (status) {
      filtered = filtered.filter((p) => p.service_status === status);
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        records: filtered.slice((pageNo - 1) * pageSize, pageNo * pageSize),
        totalCount: filtered.length,
        pageNo,
        pageSize,
        pages: Math.ceil(filtered.length / pageSize),
      },
    });
  }),

  // POST /api/projects - 创建项目
  http.post(`${BASE_URL}/projects`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const body = (await request.json()) as { git_url: string; name?: string };

    // 模拟 URL 重复
    if (body.git_url === 'https://gitlab.com/team/symphony-core') {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_001', retMsg: '项目 URL 已存在', data: null },
        { status: 409 },
      );
    }

    // 模拟无效 URL
    if (!body.git_url.startsWith('http') && !body.git_url.startsWith('git@')) {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_002', retMsg: 'Git URL 格式无效', data: null },
        { status: 400 },
      );
    }

    const newProject = createMockProject({
      id: 100,
      name: body.name || 'parsed-repo-name',
      git_url: body.git_url,
    });

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: newProject,
    });
  }),

  // GET /api/projects/:id - 项目详情
  http.get(`${BASE_URL}/projects/:id`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const id = parseInt(params.id as string);
    const project = mockProjects.find((p) => p.id === id);

    if (!project) {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_003', retMsg: '项目不存在', data: null },
        { status: 404 },
      );
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: project,
    });
  }),

  // PUT /api/projects/:id - 更新项目
  http.put(`${BASE_URL}/projects/:id`, async ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const id = parseInt(params.id as string);
    const project = mockProjects.find((p) => p.id === id);

    if (!project) {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_003', retMsg: '项目不存在', data: null },
        { status: 404 },
      );
    }

    if (project.my_role !== 'owner') {
      return HttpResponse.json(
        { success: false, retCode: 'FORBIDDEN', retMsg: '无权限修改', data: null },
        { status: 403 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // DELETE /api/projects/:id - 删除项目
  http.delete(`${BASE_URL}/projects/:id`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const id = parseInt(params.id as string);
    const project = mockProjects.find((p) => p.id === id);

    if (!project) {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_003', retMsg: '项目不存在', data: null },
        { status: 404 },
      );
    }

    if (project.service_status === 'running') {
      return HttpResponse.json(
        { success: false, retCode: 'PROJECT_004', retMsg: '服务运行中，无法删除', data: null },
        { status: 409 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),
];
```

#### `src/test/mocks/handlers-service.ts`

```typescript
import { http, HttpResponse } from 'msw';
import { serviceStatusRunning, serviceStatusStopped } from '../fixtures/service-status';

const BASE_URL = '*/api';

export const serviceHandlers = [
  // POST /api/projects/:id/start
  http.post(`${BASE_URL}/projects/:id/start`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const id = parseInt(params.id as string);
    // 模拟已运行冲突
    if (id === 1) {
      return HttpResponse.json(
        { success: false, retCode: 'SERVICE_001', retMsg: '服务已在运行', data: null },
        { status: 409 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // POST /api/projects/:id/stop
  http.post(`${BASE_URL}/projects/:id/stop`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // POST /api/projects/:id/restart
  http.post(`${BASE_URL}/projects/:id/restart`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // GET /api/projects/:id/status
  http.get(`${BASE_URL}/projects/:id/status`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const id = parseInt(params.id as string);
    const statusData = id === 1 ? serviceStatusRunning : serviceStatusStopped;

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: statusData,
    });
  }),
];
```

#### `src/test/mocks/handlers-members.ts`

```typescript
import { http, HttpResponse } from 'msw';
import { mockMembers, mockSyncResult } from '../fixtures/members';

const BASE_URL = '*/api';

export const memberHandlers = [
  // GET /api/projects/:id/members
  http.get(`${BASE_URL}/projects/:id/members`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: mockMembers,
    });
  }),

  // POST /api/projects/:id/members
  http.post(`${BASE_URL}/projects/:id/members`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const body = (await request.json()) as { user_id: number; role?: string };

    // 模拟重复添加
    if (mockMembers.some((m) => m.user_id === body.user_id)) {
      return HttpResponse.json(
        { success: false, retCode: 'MEMBER_001', retMsg: '成员已存在', data: null },
        { status: 409 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // PUT /api/projects/:id/members/:userId
  http.put(`${BASE_URL}/projects/:id/members/:userId`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // DELETE /api/projects/:id/members/:userId
  http.delete(`${BASE_URL}/projects/:id/members/:userId`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // POST /api/projects/:id/members/sync
  http.post(`${BASE_URL}/projects/:id/members/sync`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: mockSyncResult,
    });
  }),
];
```

#### `src/test/mocks/handlers-workflow.ts`

```typescript
import { http, HttpResponse } from 'msw';

const BASE_URL = '*/api';

const defaultWorkflowContent = `# Symphony Workflow

## Triggers
- On new GitLab issue assigned
- On merge request review requested

## Actions
- Analyze code changes
- Generate review comments
`;

export const workflowHandlers = [
  // GET /api/projects/:id/workflow
  http.get(`${BASE_URL}/projects/:id/workflow`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        template_mode: 'default',
        content: defaultWorkflowContent,
      },
    });
  }),

  // PUT /api/projects/:id/workflow
  http.put(`${BASE_URL}/projects/:id/workflow`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // POST /api/projects/:id/workflow/reset
  http.post(`${BASE_URL}/projects/:id/workflow/reset`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),
];
```

#### `src/test/mocks/server.ts`（扩展）

```typescript
import { setupServer } from 'msw/node';
import { handlers } from './handlers';
import { projectHandlers } from './handlers-projects';
import { serviceHandlers } from './handlers-service';
import { memberHandlers } from './handlers-members';
import { workflowHandlers } from './handlers-workflow';

export const server = setupServer(
  ...handlers,
  ...projectHandlers,
  ...serviceHandlers,
  ...memberHandlers,
  ...workflowHandlers,
);
```

### 2.3 Custom Render Wrapper（扩展 `src/test/utils.tsx`）

```typescript
import { render, RenderOptions } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../theme';
import { vi } from 'vitest';
import type { ReactElement } from 'react';

interface CustomRenderOptions extends Omit<RenderOptions, 'wrapper'> {
  initialEntries?: string[];
  /** 预设 auth store 状态 */
  authState?: {
    token?: string;
    user?: { id: number; username: string; displayName: string; role: 'admin' | 'user' };
    isAuthenticated?: boolean;
  };
}

export function renderWithProviders(
  ui: ReactElement,
  { initialEntries = ['/'], authState, ...renderOptions }: CustomRenderOptions = {},
) {
  // 如果提供了 authState，预设 localStorage
  if (authState) {
    if (authState.token) localStorage.setItem('token', authState.token);
    if (authState.user) localStorage.setItem('user', JSON.stringify(authState.user));
  }

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <ThemeProvider theme={theme}>
        <MemoryRouter initialEntries={initialEntries}>{children}</MemoryRouter>
      </ThemeProvider>
    );
  }

  return render(ui, { wrapper: Wrapper, ...renderOptions });
}

/** 预设已登录的 admin 用户 */
export function renderAsAdmin(ui: ReactElement, options: Omit<CustomRenderOptions, 'authState'> = {}) {
  return renderWithProviders(ui, {
    ...options,
    authState: {
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    },
  });
}

/** 预设已登录的普通用户 */
export function renderAsUser(ui: ReactElement, options: Omit<CustomRenderOptions, 'authState'> = {}) {
  return renderWithProviders(ui, {
    ...options,
    authState: {
      token: 'mock-token',
      user: { id: 2, username: 'john', displayName: 'John Doe', role: 'user' },
      isAuthenticated: true,
    },
  });
}

export function createMockAuthState(overrides = {}) {
  return {
    token: 'mock-token',
    user: {
      id: 1,
      username: 'admin',
      displayName: 'Administrator',
      role: 'admin' as const,
    },
    isAuthenticated: true,
    login: vi.fn(),
    logout: vi.fn(),
    setUser: vi.fn(),
    ...overrides,
  };
}
```

---

## 3. 完整测试用例清单

### 3.1 组件单元测试

#### ProjectCard (`components/projects/__tests__/ProjectCard.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders project name and description | 项目名称、描述文本正确渲染 |
| 2 | displays correct platform icon (GitLab) | GitLab 项目显示 GitLab 图标 |
| 3 | displays correct platform icon (GitHub) | GitHub 项目显示 GitHub 图标 |
| 4 | shows running status badge with green indicator | running 状态显示绿色指示 |
| 5 | shows stopped status badge with gray indicator | stopped 状态显示灰色指示 |
| 6 | shows error status badge with red indicator | error 状态显示红色指示 |
| 7 | displays member count | 成员数量正确显示 |
| 8 | shows start button when service is stopped | 停止状态显示启动按钮 |
| 9 | shows stop button when service is running | 运行状态显示停止按钮 |
| 10 | calls onStart when start button clicked | 点击启动按钮触发回调 |
| 11 | calls onStop when stop button clicked | 点击停止按钮触发回调 |
| 12 | navigates to settings on card click | 点击卡片跳转设置页 |
| 13 | hides control buttons for member role | member 角色不显示控制按钮 |
| 14 | shows namespace/repo format | 显示 namespace/repo_name 格式 |

```typescript
// 示例测试代码
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { renderAsAdmin } from '../../../test/utils';
import { ProjectCard } from '../ProjectCard';
import { mockProjectRunning, mockProjectStopped } from '../../../test/fixtures/projects';

describe('ProjectCard', () => {
  it('renders project name and description', () => {
    renderAsAdmin(<ProjectCard project={mockProjectRunning} />);
    expect(screen.getByText('symphony-core')).toBeInTheDocument();
    expect(screen.getByText('Symphony 核心引擎')).toBeInTheDocument();
  });

  it('shows running status with green indicator', () => {
    renderAsAdmin(<ProjectCard project={mockProjectRunning} />);
    const badge = screen.getByTestId('service-status-badge');
    expect(badge).toHaveTextContent(/running/i);
  });

  it('shows start button when stopped', () => {
    const onStart = vi.fn();
    renderAsAdmin(<ProjectCard project={mockProjectStopped} onStart={onStart} />);
    expect(screen.getByRole('button', { name: /启动/i })).toBeInTheDocument();
  });

  it('calls onStart callback', async () => {
    const user = userEvent.setup();
    const onStart = vi.fn();
    renderAsAdmin(<ProjectCard project={mockProjectStopped} onStart={onStart} />);
    await user.click(screen.getByRole('button', { name: /启动/i }));
    expect(onStart).toHaveBeenCalledWith(mockProjectStopped.id);
  });
});
```

#### GitUrlInput (`components/projects/__tests__/GitUrlInput.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders input field with placeholder | 输入框正确渲染 |
| 2 | shows parsed result for valid GitLab HTTPS URL | 解析 GitLab HTTPS 格式 |
| 3 | shows parsed result for valid GitHub HTTPS URL | 解析 GitHub HTTPS 格式 |
| 4 | shows parsed result for SSH URL | 解析 SSH 格式 |
| 5 | shows parsed result for custom domain | 解析自建域名 |
| 6 | shows error for invalid URL | 无效 URL 显示错误 |
| 7 | debounces input before parsing | 输入防抖 |
| 8 | calls onChange with parsed data | 解析结果通过回调传出 |
| 9 | displays platform badge after parsing | 解析后显示平台标识 |
| 10 | handles URL with trailing slash | 处理尾部斜杠 |
| 11 | handles URL with .git suffix | 处理 .git 后缀 |
| 12 | clears parsed result when input cleared | 清空输入时清除解析结果 |

```typescript
describe('GitUrlInput', () => {
  it('parses GitLab HTTPS URL and shows result', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderAsAdmin(<GitUrlInput onChange={onChange} />);

    await user.type(
      screen.getByPlaceholderText(/输入 Git 仓库地址/i),
      'https://gitlab.com/group/my-project',
    );

    await waitFor(() => {
      expect(screen.getByText('gitlab.com')).toBeInTheDocument();
      expect(screen.getByText('group/my-project')).toBeInTheDocument();
    });

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        platform: 'gitlab',
        namespace: 'group',
        repo_name: 'my-project',
      }),
    );
  });

  it('shows error for invalid URL', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<GitUrlInput onChange={vi.fn()} />);

    await user.type(screen.getByPlaceholderText(/输入 Git 仓库地址/i), 'not-a-url');

    await waitFor(() => {
      expect(screen.getByText(/URL 格式无效/i)).toBeInTheDocument();
    });
  });
});
```

#### ServiceStatusBadge (`components/projects/__tests__/ServiceStatusBadge.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders "运行中" for running status | running 状态文本 |
| 2 | renders "已停止" for stopped status | stopped 状态文本 |
| 3 | renders "启动中" for starting status | starting 状态文本 |
| 4 | renders "停止中" for stopping status | stopping 状态文本 |
| 5 | renders "异常" for error status | error 状态文本 |
| 6 | renders "已失败" for failed status | failed 状态文本 |
| 7 | applies success color for running | 运行状态使用 success 色 |
| 8 | applies error color for error/failed | 错误状态使用 error 色 |
| 9 | applies default color for stopped | 停止状态使用默认色 |
| 10 | shows pulsing animation for starting/stopping | 过渡状态显示动画 |

#### ServiceControlPanel (`components/projects/__tests__/ServiceControlPanel.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | shows start button when stopped | 停止时显示启动按钮 |
| 2 | shows stop and restart buttons when running | 运行时显示停止和重启按钮 |
| 3 | disables all buttons during starting | 启动中禁用所有按钮 |
| 4 | disables all buttons during stopping | 停止中禁用所有按钮 |
| 5 | calls onStart when start clicked | 启动回调 |
| 6 | calls onStop when stop clicked | 停止回调 |
| 7 | calls onRestart when restart clicked | 重启回调 |
| 8 | shows confirmation dialog before stop | 停止前确认对话框 |
| 9 | displays uptime when running | 运行时显示运行时长 |
| 10 | displays error message when in error state | 错误状态显示错误信息 |
| 11 | displays restart count | 显示重启次数 |
| 12 | hides controls for non-owner members | 非 owner 隐藏控制按钮 |

#### MemberTable (`components/projects/__tests__/MemberTable.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders all members in table | 渲染所有成员行 |
| 2 | displays username and display name | 显示用户名和显示名 |
| 3 | shows role chip (owner/member) | 角色 Chip 正确 |
| 4 | shows synced_from badge for synced members | 同步来源标识 |
| 5 | shows remove button for each member | 移除按钮 |
| 6 | calls onRemove with correct user_id | 移除回调参数正确 |
| 7 | shows role switch dropdown | 角色切换下拉 |
| 8 | calls onRoleChange on role switch | 角色切换回调 |
| 9 | hides actions for non-owner users | 非 owner 隐藏操作列 |
| 10 | cannot remove self (owner) | 不能移除自己 |
| 11 | shows empty state when no members | 空状态提示 |

#### WorkflowEditor (`components/projects/__tests__/WorkflowEditor.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders workflow content in read mode | 只读模式渲染内容 |
| 2 | switches to edit mode on edit button click | 点击编辑进入编辑模式 |
| 3 | shows textarea in edit mode | 编辑模式显示文本域 |
| 4 | calls onSave with updated content | 保存回调传递新内容 |
| 5 | calls onCancel and reverts changes | 取消恢复原内容 |
| 6 | shows template mode indicator (default/custom) | 模板模式指示 |
| 7 | shows reset button in custom mode | 自定义模式显示重置按钮 |
| 8 | calls onReset when reset clicked | 重置回调 |
| 9 | shows confirmation before reset | 重置前确认 |
| 10 | disables edit for non-owner | 非 owner 禁用编辑 |

### 3.2 Store 测试

#### projectStore (`store/__tests__/projectStore.test.ts`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | fetchProjects sets projects array | 获取列表更新 state |
| 2 | fetchProjects sets loading during request | 请求中 loading=true |
| 3 | fetchProjects handles API error | API 错误处理 |
| 4 | createProject adds to projects list | 创建后更新列表 |
| 5 | createProject handles duplicate URL error | 重复 URL 错误 |
| 6 | startService updates project status to starting | 启动更新状态 |
| 7 | stopService updates project status to stopping | 停止更新状态 |
| 8 | restartService calls stop then start | 重启流程 |
| 9 | fetchProjectDetail sets currentProject | 获取详情 |
| 10 | updateProject updates project in list | 更新项目 |
| 11 | deleteProject removes from list | 删除项目 |
| 12 | filter by platform works correctly | 平台筛选 |
| 13 | filter by status works correctly | 状态筛选 |
| 14 | search filter works correctly | 搜索筛选 |

```typescript
import { describe, it, expect, beforeEach } from 'vitest';
import { useProjectStore } from '../../store/projectStore';

describe('projectStore', () => {
  beforeEach(() => {
    useProjectStore.setState({
      projects: [],
      currentProject: null,
      loading: false,
      error: null,
    });
    localStorage.setItem('token', 'mock-token');
  });

  it('fetchProjects populates projects array', async () => {
    await useProjectStore.getState().fetchProjects();
    const state = useProjectStore.getState();
    expect(state.projects.length).toBeGreaterThan(0);
    expect(state.loading).toBe(false);
  });

  it('fetchProjects sets loading during request', async () => {
    const promise = useProjectStore.getState().fetchProjects();
    expect(useProjectStore.getState().loading).toBe(true);
    await promise;
    expect(useProjectStore.getState().loading).toBe(false);
  });

  it('startService updates status optimistically', async () => {
    useProjectStore.setState({
      projects: [{ id: 2, service_status: 'stopped' } as any],
    });
    await useProjectStore.getState().startService(2);
    // 乐观更新后状态应变为 starting 或 running
    const project = useProjectStore.getState().projects.find((p) => p.id === 2);
    expect(['starting', 'running']).toContain(project?.service_status);
  });
});
```

### 3.3 工具函数测试

#### Git URL 解析 (`utils/__tests__/parseGitUrl.test.ts`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | parses GitLab HTTPS URL | `https://gitlab.com/group/project` |
| 2 | parses GitLab multi-level namespace | `https://gitlab.com/group/sub/project` |
| 3 | parses GitHub HTTPS URL | `https://github.com/owner/repo` |
| 4 | parses GitLab SSH URL | `git@gitlab.com:group/project.git` |
| 5 | parses GitHub SSH URL | `git@github.com:owner/repo.git` |
| 6 | parses custom domain GitLab | `https://gitlab.example.com/group/project` |
| 7 | handles trailing slash | `https://gitlab.com/group/project/` |
| 8 | handles .git suffix | `https://github.com/owner/repo.git` |
| 9 | returns null for invalid URL | `not-a-url` |
| 10 | returns null for empty string | `` |
| 11 | identifies platform from domain | github.com → github, gitlab → gitlab |

#### 状态格式化 (`utils/__tests__/formatStatus.test.ts`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | formats running status | '运行中' |
| 2 | formats stopped status | '已停止' |
| 3 | formats error status | '异常' |
| 4 | formats uptime seconds to human readable | 3600 → '1小时' |
| 5 | formats date to relative time | ISO string → '2小时前' |

### 3.4 页面集成测试

#### ProjectListPage (`pages/projects/__tests__/ProjectListPage.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders project list after loading | 加载后显示项目列表 |
| 2 | shows loading skeleton during fetch | 加载中显示骨架屏 |
| 3 | shows empty state when no projects | 无项目时空状态 |
| 4 | filters by platform (GitLab/GitHub) | 平台筛选 |
| 5 | filters by service status | 状态筛选 |
| 6 | search filters projects by name | 搜索过滤 |
| 7 | toggles between card and list view | 视图切换 |
| 8 | navigates to create page on button click | 跳转创建页 |
| 9 | start service from quick action | 快速启动 |
| 10 | stop service from quick action | 快速停止 |
| 11 | shows error toast on API failure | API 失败提示 |
| 12 | pagination works correctly | 分页功能 |
| 13 | admin sees all projects | admin 全量可见 |

```typescript
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { renderAsAdmin } from '../../../test/utils';
import ProjectListPage from '../ProjectListPage';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return { ...actual, useNavigate: () => mockNavigate };
});

describe('ProjectListPage', () => {
  it('renders project list after loading', async () => {
    renderAsAdmin(<ProjectListPage />, { initialEntries: ['/projects'] });

    await waitFor(() => {
      expect(screen.getByText('symphony-core')).toBeInTheDocument();
      expect(screen.getByText('web-frontend')).toBeInTheDocument();
    });
  });

  it('shows empty state when no projects', async () => {
    // Override handler to return empty list
    server.use(
      http.get('*/api/projects', () =>
        HttpResponse.json({
          success: true, retCode: 'SUCCESS', retMsg: 'ok',
          data: { records: [], totalCount: 0, pageNo: 1, pageSize: 20, pages: 0 },
        }),
      ),
    );

    renderAsAdmin(<ProjectListPage />, { initialEntries: ['/projects'] });

    await waitFor(() => {
      expect(screen.getByText(/暂无项目/i)).toBeInTheDocument();
    });
  });

  it('navigates to create page', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<ProjectListPage />, { initialEntries: ['/projects'] });

    await waitFor(() => screen.getByText('symphony-core'));
    await user.click(screen.getByRole('button', { name: /创建项目/i }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects/new');
  });
});
```

#### CreateProjectPage (`pages/projects/__tests__/CreateProjectPage.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders form with Git URL input | 表单渲染 |
| 2 | shows parsed URL preview after input | URL 解析预览 |
| 3 | auto-fills project name from repo name | 自动填充名称 |
| 4 | allows manual name override | 手动覆盖名称 |
| 5 | shows platform detection result | 平台识别结果 |
| 6 | validates required fields on submit | 必填校验 |
| 7 | submits form and navigates to settings | 提交后跳转 |
| 8 | shows error on duplicate URL | 重复 URL 错误 |
| 9 | shows error on invalid URL | 无效 URL 错误 |
| 10 | shows loading state during submission | 提交中加载状态 |
| 11 | cancel button navigates back | 取消返回 |

#### ProjectSettingsPage (`pages/projects/__tests__/ProjectSettingsPage.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | loads and displays project info | 加载项目信息 |
| 2 | edits project name and saves | 编辑名称保存 |
| 3 | edits project description and saves | 编辑描述保存 |
| 4 | shows service control panel | 服务控制面板 |
| 5 | shows workflow editor tab | 工作流编辑器 Tab |
| 6 | switches between tabs | Tab 切换 |
| 7 | shows delete button for owner | owner 显示删除 |
| 8 | shows delete confirmation dialog | 删除确认对话框 |
| 9 | prevents delete when service running | 运行中禁止删除 |
| 10 | shows 403 error for non-owner | 非 owner 权限错误 |
| 11 | shows 404 for non-existent project | 项目不存在 |

#### ProjectMembersPage (`pages/projects/__tests__/ProjectMembersPage.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | loads and displays member list | 加载成员列表 |
| 2 | shows add member button | 添加成员按钮 |
| 3 | opens add member dialog | 打开添加对话框 |
| 4 | adds member successfully | 成功添加成员 |
| 5 | shows error on duplicate member | 重复添加错误 |
| 6 | removes member with confirmation | 确认后移除成员 |
| 7 | changes member role | 切换成员角色 |
| 8 | syncs members from platform | 平台同步 |
| 9 | shows sync result summary | 同步结果摘要 |
| 10 | shows unmatched users from sync | 未匹配用户列表 |
| 11 | hides actions for non-owner | 非 owner 隐藏操作 |
| 12 | shows permission denied for unauthorized | 无权限提示 |

---

## 4. E2E 测试（Playwright）

### 4.1 Playwright 配置

#### `playwright.config.ts`

```typescript
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI
    ? [['html', { open: 'never' }], ['junit', { outputFile: 'test-results/e2e-results.xml' }]]
    : 'html',
  use: {
    baseURL: process.env.E2E_BASE_URL || 'http://localhost:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
  ],
  webServer: process.env.CI
    ? undefined
    : {
        command: 'npm run dev',
        url: 'http://localhost:5173',
        reuseExistingServer: !process.env.CI,
      },
});
```

### 4.2 Page Objects

#### `e2e/page-objects/BasePage.ts`

```typescript
import { Page, Locator } from '@playwright/test';

export class BasePage {
  readonly page: Page;
  readonly sidebar: Locator;
  readonly topNav: Locator;

  constructor(page: Page) {
    this.page = page;
    this.sidebar = page.locator('[data-testid="sidebar"]');
    this.topNav = page.locator('[data-testid="top-nav"]');
  }

  async navigateTo(path: string) {
    await this.page.goto(path);
  }

  async waitForPageLoad() {
    await this.page.waitForLoadState('networkidle');
  }

  async getToastMessage(): Promise<string> {
    const toast = this.page.locator('[role="alert"]');
    await toast.waitFor({ state: 'visible' });
    return toast.textContent() ?? '';
  }
}
```

#### `e2e/page-objects/LoginPage.ts`

```typescript
import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

export class LoginPage extends BasePage {
  readonly usernameInput: Locator;
  readonly passwordInput: Locator;
  readonly submitButton: Locator;
  readonly errorAlert: Locator;

  constructor(page: Page) {
    super(page);
    this.usernameInput = page.getByLabel('用户名');
    this.passwordInput = page.getByLabel('密码');
    this.submitButton = page.getByRole('button', { name: '登录' });
    this.errorAlert = page.getByRole('alert');
  }

  async goto() {
    await this.navigateTo('/login');
  }

  async login(username: string, password: string) {
    await this.usernameInput.fill(username);
    await this.passwordInput.fill(password);
    await this.submitButton.click();
  }

  async loginAsAdmin() {
    await this.login('admin', 'Admin@123456');
    await this.page.waitForURL('**/projects');
  }

  async loginAsUser() {
    await this.login('john', 'User@123456');
    await this.page.waitForURL('**/projects');
  }
}
```

#### `e2e/page-objects/ProjectListPage.ts`

```typescript
import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

export class ProjectListPage extends BasePage {
  readonly createButton: Locator;
  readonly searchInput: Locator;
  readonly platformFilter: Locator;
  readonly statusFilter: Locator;
  readonly viewToggle: Locator;
  readonly projectCards: Locator;
  readonly emptyState: Locator;

  constructor(page: Page) {
    super(page);
    this.createButton = page.getByRole('button', { name: /创建项目/i });
    this.searchInput = page.getByPlaceholder(/搜索项目/i);
    this.platformFilter = page.getByTestId('platform-filter');
    this.statusFilter = page.getByTestId('status-filter');
    this.viewToggle = page.getByTestId('view-toggle');
    this.projectCards = page.locator('[data-testid="project-card"]');
    this.emptyState = page.getByTestId('empty-state');
  }

  async goto() {
    await this.navigateTo('/projects');
  }

  async getProjectCount(): Promise<number> {
    return this.projectCards.count();
  }

  async clickProject(name: string) {
    await this.page.getByText(name).click();
  }

  async startProject(name: string) {
    const card = this.page.locator('[data-testid="project-card"]', { hasText: name });
    await card.getByRole('button', { name: /启动/i }).click();
  }

  async stopProject(name: string) {
    const card = this.page.locator('[data-testid="project-card"]', { hasText: name });
    await card.getByRole('button', { name: /停止/i }).click();
  }

  async searchProjects(query: string) {
    await this.searchInput.fill(query);
  }

  async filterByPlatform(platform: 'gitlab' | 'github') {
    await this.platformFilter.click();
    await this.page.getByRole('option', { name: new RegExp(platform, 'i') }).click();
  }

  async filterByStatus(status: string) {
    await this.statusFilter.click();
    await this.page.getByRole('option', { name: new RegExp(status, 'i') }).click();
  }
}
```

#### `e2e/page-objects/CreateProjectPage.ts`

```typescript
import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

export class CreateProjectPage extends BasePage {
  readonly gitUrlInput: Locator;
  readonly nameInput: Locator;
  readonly descriptionInput: Locator;
  readonly platformBadge: Locator;
  readonly parsedPreview: Locator;
  readonly submitButton: Locator;
  readonly cancelButton: Locator;

  constructor(page: Page) {
    super(page);
    this.gitUrlInput = page.getByPlaceholder(/输入 Git 仓库地址/i);
    this.nameInput = page.getByLabel(/项目名称/i);
    this.descriptionInput = page.getByLabel(/项目描述/i);
    this.platformBadge = page.getByTestId('platform-badge');
    this.parsedPreview = page.getByTestId('parsed-preview');
    this.submitButton = page.getByRole('button', { name: /创建/i });
    this.cancelButton = page.getByRole('button', { name: /取消/i });
  }

  async goto() {
    await this.navigateTo('/projects/new');
  }

  async fillGitUrl(url: string) {
    await this.gitUrlInput.fill(url);
    // 等待解析完成
    await this.parsedPreview.waitFor({ state: 'visible', timeout: 3000 });
  }

  async fillForm(data: { gitUrl: string; name?: string; description?: string }) {
    await this.fillGitUrl(data.gitUrl);
    if (data.name) {
      await this.nameInput.clear();
      await this.nameInput.fill(data.name);
    }
    if (data.description) {
      await this.descriptionInput.fill(data.description);
    }
  }

  async submit() {
    await this.submitButton.click();
  }
}
```

#### `e2e/page-objects/ProjectSettingsPage.ts`

```typescript
import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

export class ProjectSettingsPage extends BasePage {
  readonly nameInput: Locator;
  readonly descriptionInput: Locator;
  readonly saveButton: Locator;
  readonly deleteButton: Locator;
  readonly servicePanel: Locator;
  readonly workflowTab: Locator;
  readonly infoTab: Locator;

  constructor(page: Page) {
    super(page);
    this.nameInput = page.getByLabel(/项目名称/i);
    this.descriptionInput = page.getByLabel(/项目描述/i);
    this.saveButton = page.getByRole('button', { name: /保存/i });
    this.deleteButton = page.getByRole('button', { name: /删除项目/i });
    this.servicePanel = page.getByTestId('service-control-panel');
    this.workflowTab = page.getByRole('tab', { name: /工作流/i });
    this.infoTab = page.getByRole('tab', { name: /基本信息/i });
  }

  async goto(projectId: number) {
    await this.navigateTo(`/projects/${projectId}/settings`);
  }

  async updateName(name: string) {
    await this.nameInput.clear();
    await this.nameInput.fill(name);
    await this.saveButton.click();
  }

  async deleteProject() {
    await this.deleteButton.click();
    await this.page.getByRole('button', { name: /确认删除/i }).click();
  }

  async switchToWorkflowTab() {
    await this.workflowTab.click();
  }
}
```

#### `e2e/page-objects/ProjectMembersPage.ts`

```typescript
import { Page, Locator } from '@playwright/test';
import { BasePage } from './BasePage';

export class ProjectMembersPage extends BasePage {
  readonly addButton: Locator;
  readonly syncButton: Locator;
  readonly memberRows: Locator;
  readonly addDialog: Locator;

  constructor(page: Page) {
    super(page);
    this.addButton = page.getByRole('button', { name: /添加成员/i });
    this.syncButton = page.getByRole('button', { name: /同步/i });
    this.memberRows = page.locator('[data-testid="member-row"]');
    this.addDialog = page.getByRole('dialog');
  }

  async goto(projectId: number) {
    await this.navigateTo(`/projects/${projectId}/members`);
  }

  async getMemberCount(): Promise<number> {
    return this.memberRows.count();
  }

  async addMember(username: string, role: 'owner' | 'member' = 'member') {
    await this.addButton.click();
    await this.addDialog.getByPlaceholder(/搜索用户/i).fill(username);
    await this.page.getByRole('option', { name: new RegExp(username, 'i') }).click();
    if (role === 'owner') {
      await this.addDialog.getByLabel(/角色/i).click();
      await this.page.getByRole('option', { name: /owner/i }).click();
    }
    await this.addDialog.getByRole('button', { name: /确认/i }).click();
  }

  async removeMember(username: string) {
    const row = this.page.locator('[data-testid="member-row"]', { hasText: username });
    await row.getByRole('button', { name: /移除/i }).click();
    await this.page.getByRole('button', { name: /确认/i }).click();
  }

  async changeMemberRole(username: string, newRole: 'owner' | 'member') {
    const row = this.page.locator('[data-testid="member-row"]', { hasText: username });
    await row.getByTestId('role-select').click();
    await this.page.getByRole('option', { name: new RegExp(newRole, 'i') }).click();
  }

  async syncFromPlatform() {
    await this.syncButton.click();
  }
}
```

### 4.3 E2E 测试用例

#### `e2e/project-crud.spec.ts` - 项目 CRUD 流程

```typescript
import { test, expect } from '@playwright/test';
import { LoginPage } from './page-objects/LoginPage';
import { ProjectListPage } from './page-objects/ProjectListPage';
import { CreateProjectPage } from './page-objects/CreateProjectPage';
import { ProjectSettingsPage } from './page-objects/ProjectSettingsPage';

test.describe('Project CRUD Flow', () => {
  let loginPage: LoginPage;
  let listPage: ProjectListPage;
  let createPage: CreateProjectPage;
  let settingsPage: ProjectSettingsPage;

  test.beforeEach(async ({ page }) => {
    loginPage = new LoginPage(page);
    listPage = new ProjectListPage(page);
    createPage = new CreateProjectPage(page);
    settingsPage = new ProjectSettingsPage(page);

    await loginPage.goto();
    await loginPage.loginAsAdmin();
  });

  test('full project creation flow', async ({ page }) => {
    // 1. 从列表页进入创建页
    await listPage.goto();
    await listPage.createButton.click();
    await expect(page).toHaveURL(/\/projects\/new/);

    // 2. 输入 Git URL，验证解析
    await createPage.fillGitUrl('https://gitlab.com/myteam/new-project');
    await expect(createPage.platformBadge).toContainText('GitLab');
    await expect(createPage.parsedPreview).toContainText('myteam/new-project');

    // 3. 项目名称自动填充
    await expect(createPage.nameInput).toHaveValue('new-project');

    // 4. 提交创建
    await createPage.submit();

    // 5. 跳转到项目设置页
    await expect(page).toHaveURL(/\/projects\/\d+\/settings/);
  });

  test('project creation with duplicate URL shows error', async () => {
    await createPage.goto();
    await createPage.fillGitUrl('https://gitlab.com/team/symphony-core');
    await createPage.submit();

    const toast = await createPage.getToastMessage();
    expect(toast).toContain('项目 URL 已存在');
  });

  test('project list displays all projects', async () => {
    await listPage.goto();
    const count = await listPage.getProjectCount();
    expect(count).toBeGreaterThan(0);
  });

  test('project search filters results', async () => {
    await listPage.goto();
    await listPage.searchProjects('symphony');

    await expect(listPage.projectCards.first()).toContainText('symphony');
  });

  test('project settings update', async ({ page }) => {
    await settingsPage.goto(1);
    await settingsPage.updateName('symphony-core-renamed');

    const toast = await settingsPage.getToastMessage();
    expect(toast).toContain('保存成功');
  });

  test('project delete requires confirmation', async ({ page }) => {
    await settingsPage.goto(2); // stopped project
    await settingsPage.deleteButton.click();

    // 确认对话框出现
    await expect(page.getByText(/确认删除/i)).toBeVisible();
  });
});
```

#### `e2e/service-management.spec.ts` - 服务管理流程

```typescript
import { test, expect } from '@playwright/test';
import { LoginPage } from './page-objects/LoginPage';
import { ProjectListPage } from './page-objects/ProjectListPage';
import { ProjectSettingsPage } from './page-objects/ProjectSettingsPage';

test.describe('Service Management Flow', () => {
  let loginPage: LoginPage;
  let listPage: ProjectListPage;
  let settingsPage: ProjectSettingsPage;

  test.beforeEach(async ({ page }) => {
    loginPage = new LoginPage(page);
    listPage = new ProjectListPage(page);
    settingsPage = new ProjectSettingsPage(page);

    await loginPage.goto();
    await loginPage.loginAsAdmin();
  });

  test('start service from project list', async ({ page }) => {
    await listPage.goto();
    await listPage.startProject('web-frontend'); // stopped project

    // 验证状态变化
    await expect(
      page.locator('[data-testid="project-card"]', { hasText: 'web-frontend' })
        .locator('[data-testid="service-status-badge"]')
    ).not.toContainText('已停止');
  });

  test('stop service from project settings', async ({ page }) => {
    await settingsPage.goto(1); // running project

    const stopButton = page.getByRole('button', { name: /停止/i });
    await stopButton.click();

    // 确认对话框
    await page.getByRole('button', { name: /确认/i }).click();

    const toast = await settingsPage.getToastMessage();
    expect(toast).toContain('停止');
  });

  test('cannot start already running service', async ({ page }) => {
    await settingsPage.goto(1); // running project

    // 启动按钮应该不可见或禁用
    const startButton = page.getByRole('button', { name: /启动/i });
    await expect(startButton).not.toBeVisible();
  });

  test('service status displays uptime', async ({ page }) => {
    await settingsPage.goto(1); // running project
    await expect(page.getByTestId('service-uptime')).toBeVisible();
  });

  test('error state shows error message', async ({ page }) => {
    await settingsPage.goto(3); // error project
    await expect(page.getByTestId('error-message')).toBeVisible();
  });
});
```

#### `e2e/member-management.spec.ts` - 成员管理流程

```typescript
import { test, expect } from '@playwright/test';
import { LoginPage } from './page-objects/LoginPage';
import { ProjectMembersPage } from './page-objects/ProjectMembersPage';

test.describe('Member Management Flow', () => {
  let loginPage: LoginPage;
  let membersPage: ProjectMembersPage;

  test.beforeEach(async ({ page }) => {
    loginPage = new LoginPage(page);
    membersPage = new ProjectMembersPage(page);

    await loginPage.goto();
    await loginPage.loginAsAdmin();
  });

  test('displays member list', async () => {
    await membersPage.goto(1);
    const count = await membersPage.getMemberCount();
    expect(count).toBeGreaterThan(0);
  });

  test('add member flow', async ({ page }) => {
    await membersPage.goto(1);
    await membersPage.addMember('newuser');

    const toast = await membersPage.getToastMessage();
    expect(toast).toContain('添加成功');
  });

  test('remove member with confirmation', async ({ page }) => {
    await membersPage.goto(1);
    await membersPage.removeMember('john');

    const toast = await membersPage.getToastMessage();
    expect(toast).toContain('移除成功');
  });

  test('change member role', async ({ page }) => {
    await membersPage.goto(1);
    await membersPage.changeMemberRole('john', 'owner');

    const toast = await membersPage.getToastMessage();
    expect(toast).toContain('更新成功');
  });

  test('sync members from platform', async ({ page }) => {
    await membersPage.goto(1);
    await membersPage.syncFromPlatform();

    // 验证同步结果摘要
    await expect(page.getByText(/同步完成/i)).toBeVisible();
    await expect(page.getByText(/添加.*2/i)).toBeVisible();
  });
});
```

#### `e2e/permission.spec.ts` - 权限验证流程

```typescript
import { test, expect } from '@playwright/test';
import { LoginPage } from './page-objects/LoginPage';
import { ProjectListPage } from './page-objects/ProjectListPage';
import { ProjectSettingsPage } from './page-objects/ProjectSettingsPage';
import { ProjectMembersPage } from './page-objects/ProjectMembersPage';

test.describe('Permission-based UI', () => {
  test('regular user cannot see admin-only projects', async ({ page }) => {
    const loginPage = new LoginPage(page);
    const listPage = new ProjectListPage(page);

    await loginPage.goto();
    await loginPage.loginAsUser();
    await listPage.goto();

    // 普通用户只能看到自己参与的项目
    const count = await listPage.getProjectCount();
    expect(count).toBeLessThanOrEqual(2); // 只有 member 角色的项目
  });

  test('member cannot access service controls', async ({ page }) => {
    const loginPage = new LoginPage(page);
    const settingsPage = new ProjectSettingsPage(page);

    await loginPage.goto();
    await loginPage.loginAsUser();
    await settingsPage.goto(2); // project where user is member

    // 服务控制按钮不可见
    await expect(page.getByRole('button', { name: /启动/i })).not.toBeVisible();
    await expect(page.getByRole('button', { name: /停止/i })).not.toBeVisible();
  });

  test('member cannot manage other members', async ({ page }) => {
    const loginPage = new LoginPage(page);
    const membersPage = new ProjectMembersPage(page);

    await loginPage.goto();
    await loginPage.loginAsUser();
    await membersPage.goto(2);

    // 添加/移除按钮不可见
    await expect(membersPage.addButton).not.toBeVisible();
  });

  test('admin can access all project settings', async ({ page }) => {
    const loginPage = new LoginPage(page);
    const settingsPage = new ProjectSettingsPage(page);

    await loginPage.goto();
    await loginPage.loginAsAdmin();
    await settingsPage.goto(2); // any project

    // Admin 可以看到所有控制
    await expect(settingsPage.saveButton).toBeVisible();
    await expect(settingsPage.deleteButton).toBeVisible();
  });

  test('unauthenticated user redirected to login', async ({ page }) => {
    await page.goto('/projects');
    await expect(page).toHaveURL(/\/login/);
  });
});
```

---

## 5. GitLab CI Pipeline 配置

### `.gitlab-ci.yml`（前端测试部分）

```yaml
stages:
  - install
  - lint
  - test-unit
  - test-e2e
  - report

variables:
  NODE_VERSION: "20"
  CI_REGISTRY: http://gitlab.jushuitan-inc.com:8081
  PROJECT_PATH: zimei10525/symphony_e2e_test_repo

# ============================================================
# Stage: Install
# ============================================================
install-frontend:
  stage: install
  image: node:${NODE_VERSION}-alpine
  cache:
    key:
      files:
        - web-frontend/package-lock.json
    paths:
      - web-frontend/node_modules/
  script:
    - cd web-frontend
    - npm ci --prefer-offline
  artifacts:
    paths:
      - web-frontend/node_modules/
    expire_in: 1 hour

# ============================================================
# Stage: Lint
# ============================================================
lint-frontend:
  stage: lint
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npm run lint
  allow_failure: false

typecheck-frontend:
  stage: lint
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npx tsc --noEmit
  allow_failure: false

# ============================================================
# Stage: Unit & Integration Tests
# ============================================================
test-unit-frontend:
  stage: test-unit
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npm run test:coverage
  artifacts:
    when: always
    paths:
      - web-frontend/coverage/
    reports:
      coverage_report:
        coverage_format: cobertura
        path: web-frontend/coverage/cobertura-coverage.xml
      junit: web-frontend/test-results/unit-results.xml
  coverage: '/All files[^|]*\|[^|]*\s+([\d\.]+)/'

# ============================================================
# Stage: E2E Tests
# ============================================================
test-e2e-frontend:
  stage: test-e2e
  image: mcr.microsoft.com/playwright:v1.44.0-jammy
  needs: [install-frontend]
  variables:
    E2E_BASE_URL: "http://localhost:5173"
  services:
    - name: node:${NODE_VERSION}-alpine
      alias: frontend-dev
  before_script:
    - cd web-frontend
    - npm ci --prefer-offline
    - npx playwright install --with-deps chromium
  script:
    - cd web-frontend
    # 启动 dev server 后台运行
    - npm run dev &
    - npx wait-on http://localhost:5173 --timeout 30000
    # 运行 E2E 测试
    - npx playwright test --project=chromium
  artifacts:
    when: always
    paths:
      - web-frontend/test-results/
      - web-frontend/playwright-report/
    reports:
      junit: web-frontend/test-results/e2e-results.xml
    expire_in: 7 days
  retry:
    max: 1
    when: script_failure

# ============================================================
# Stage: Report
# ============================================================
pages:
  stage: report
  needs: [test-unit-frontend, test-e2e-frontend]
  script:
    - mkdir -p public
    - cp -r web-frontend/coverage/lcov-report public/coverage || true
    - cp -r web-frontend/playwright-report public/e2e-report || true
  artifacts:
    paths:
      - public
  only:
    - main
```

### CI 触发规则

```yaml
# 仅在前端文件变更时触发前端测试
.frontend-changes: &frontend-changes
  changes:
    - web-frontend/**/*
    - .gitlab-ci.yml

# 应用到各 job
install-frontend:
  rules:
    - <<: *frontend-changes
      when: always
```

---

## 6. 测试执行命令

### 本地开发

```bash
# 运行所有单元/集成测试
cd web-frontend && npm run test:run

# 运行测试（watch 模式）
cd web-frontend && npm run test

# 运行带覆盖率的测试
cd web-frontend && npm run test:coverage

# 运行特定文件的测试
cd web-frontend && npx vitest run src/components/projects/__tests__/ProjectCard.test.tsx

# 运行匹配模式的测试
cd web-frontend && npx vitest run --grep "ProjectCard"

# 运行 E2E 测试（需要先启动 dev server）
cd web-frontend && npm run test:e2e

# 运行特定 E2E 测试文件
cd web-frontend && npx playwright test e2e/project-crud.spec.ts

# 运行 E2E 测试（带 UI 模式）
cd web-frontend && npx playwright test --ui

# 运行 E2E 测试（headed 模式，可视化调试）
cd web-frontend && npx playwright test --headed

# 生成 E2E 测试报告
cd web-frontend && npx playwright show-report
```

### CI 环境

```bash
# CI 中运行单元测试（带 JUnit 报告）
cd web-frontend && npx vitest run --reporter=junit --outputFile=test-results/unit-results.xml

# CI 中运行 E2E 测试
cd web-frontend && npx playwright test --project=chromium --reporter=junit
```

### package.json scripts 扩展

```json
{
  "scripts": {
    "test": "vitest",
    "test:run": "vitest run",
    "test:coverage": "vitest run --coverage",
    "test:e2e": "playwright test",
    "test:e2e:ui": "playwright test --ui",
    "test:e2e:headed": "playwright test --headed",
    "test:e2e:report": "playwright show-report",
    "test:ci": "vitest run --reporter=junit --outputFile=test-results/unit-results.xml && playwright test --project=chromium"
  }
}
```

---

## 7. 覆盖率目标

| 层级 | 目标覆盖率 | 说明 |
|------|-----------|------|
| 组件单元测试 | >= 80% | 所有新组件的 branches/statements |
| Store 测试 | >= 90% | 所有 actions 和 selectors |
| 工具函数 | >= 95% | 纯函数全覆盖 |
| 页面集成测试 | >= 70% | 主要用户路径 |
| E2E 测试 | 核心流程 100% | 创建/启停/成员管理 |

### Vitest 覆盖率配置（扩展 `vitest.config.ts`）

```typescript
coverage: {
  provider: 'v8',
  reporter: ['text', 'lcov', 'cobertura'],
  include: ['src/**/*.{ts,tsx}'],
  exclude: [
    'src/test/**',
    'src/**/*.test.{ts,tsx}',
    'src/**/*.d.ts',
    'src/main.tsx',
    'src/vite-env.d.ts',
  ],
  thresholds: {
    branches: 70,
    functions: 75,
    lines: 75,
    statements: 75,
  },
},
```

---

## 8. 测试数据管理策略

### 单元/集成测试
- 使用 `src/test/fixtures/` 中的静态 mock 数据
- MSW handlers 提供 API 层 mock
- 每个测试用例独立，通过 `server.use()` 覆盖默认 handler 实现特殊场景

### E2E 测试
- **方案 A（推荐）**：使用独立的测试后端实例 + SQLite 内存数据库，每次测试前重置
- **方案 B**：使用 MSW 的 browser integration 在浏览器层拦截请求
- 测试数据通过 `e2e/fixtures/test-data.ts` 管理
- 每个 test suite 使用 `test.beforeEach` 重置状态

### 环境隔离

```typescript
// e2e/fixtures/test-data.ts
export const TEST_ADMIN = {
  username: 'admin',
  password: 'Admin@123456',
};

export const TEST_USER = {
  username: 'john',
  password: 'User@123456',
};

export const TEST_GIT_URLS = {
  validGitlab: 'https://gitlab.com/test-org/test-project',
  validGithub: 'https://github.com/test-org/test-repo',
  validSsh: 'git@gitlab.com:test-org/test-project.git',
  invalid: 'not-a-valid-url',
  duplicate: 'https://gitlab.com/team/symphony-core',
};
```

---

## 9. 设计系统验证要点

测试中需要验证的 Architectural Logic 设计系统元素：

| 元素 | 验证方式 | 对应设计规范 |
|------|----------|-------------|
| 服务状态颜色 | `toHaveStyle` / CSS class 检查 | success=#2e7d32, error=#ba1a1a |
| 按钮圆角 | 视觉回归 / snapshot | 4px border-radius |
| 卡片圆角 | 视觉回归 / snapshot | 8px border-radius |
| 间距 | Layout 测试 | 4px base unit |
| 字体 | Computed style 检查 | Inter font-family |
| 侧边栏宽度 | Layout 测试 | 256px |
| 渐变按钮 | 视觉回归 | primary gradient |

---

## 10. 依赖安装

Phase 2 测试框架需要额外安装的依赖：

```bash
cd web-frontend

# Playwright（如果尚未安装）
npm install -D @playwright/test

# 安装浏览器
npx playwright install

# wait-on（CI 中等待 dev server 启动）
npm install -D wait-on
```

`package.json` devDependencies 新增：

```json
{
  "devDependencies": {
    "@playwright/test": "^1.44.0",
    "wait-on": "^7.2.0"
  }
}
```
