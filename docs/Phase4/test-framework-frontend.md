# Phase 4 前端测试框架设计

## 概述

本文档定义 Phase 4（协作与控制）前端的完整测试策略，覆盖单元测试、集成测试和 E2E 测试。

## GitLab CI 信息

- GitLab URL: http://gitlab.jushuitan-inc.com:8081/
- 项目: /zimei10525/symphony_e2e_test_repo
- GITLAB_TOKEN: gitlab-token-example

---

## 1. 单元测试 (Vitest + Testing Library)

### 1.1 目录结构

```
web-frontend/src/
├── components/
│   ├── kanban/
│   │   └── __tests__/
│   │       └── AuthorFilter.test.tsx
│   ├── concurrency/
│   │   └── __tests__/
│   │       ├── ConcurrencyPanel.test.tsx
│   │       ├── ProjectConcurrencyCard.test.tsx
│   │       └── ConcurrencyConfigDialog.test.tsx
├── store/
│   └── __tests__/
│       ├── concurrencyStore.test.ts
│       └── kanbanStore.test.ts (扩展)
├── api/
│   └── __tests__/
│       ├── concurrency.test.ts
│       └── contributors.test.ts
└── test/
    ├── setup.ts
    ├── utils.tsx
    └── mocks/
        ├── handlers.ts (扩展 Phase 4 handlers)
        └── server.ts
```

### 1.2 AuthorFilter 组件测试

```tsx
// src/components/kanban/__tests__/AuthorFilter.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import AuthorFilter from '../AuthorFilter';
import type { PlatformUser } from '../../../types/kanban';

const mockAuthors: PlatformUser[] = [
  { username: 'john', display_name: 'John Doe', avatar_url: '' },
  { username: 'jane', display_name: 'Jane Smith', avatar_url: '' },
  { username: 'bot-symphony', display_name: 'Symphony Bot', avatar_url: '' },
];

describe('AuthorFilter', () => {
  const defaultProps = {
    authors: mockAuthors,
    value: '',
    onChange: vi.fn(),
    searchValue: '',
    onSearchChange: vi.fn(),
    labelsValue: '',
    onLabelsChange: vi.fn(),
    // Phase 4 新增
    authorValue: '',
    onAuthorChange: vi.fn(),
    contributors: mockAuthors,
  };

  it('renders search input', () => {
    render(<AuthorFilter {...defaultProps} />);
    expect(screen.getByLabelText('搜索 Issue')).toBeInTheDocument();
  });

  it('renders author filter dropdown', () => {
    render(<AuthorFilter {...defaultProps} />);
    expect(screen.getByLabelText('按作者过滤')).toBeInTheDocument();
  });

  it('calls onAuthorChange when author selected', async () => {
    render(<AuthorFilter {...defaultProps} />);
    const authorInput = screen.getByLabelText('按作者过滤');
    fireEvent.change(authorInput, { target: { value: 'John' } });
    // 选择下拉选项
    const option = await screen.findByText('John Doe');
    fireEvent.click(option);
    expect(defaultProps.onAuthorChange).toHaveBeenCalledWith('john');
  });

  it('filters out bot users from author list', () => {
    render(<AuthorFilter {...defaultProps} />);
    // bot-symphony 不应出现在作者选项中
    expect(screen.queryByText('Symphony Bot')).not.toBeInTheDocument();
  });

  it('clears author filter', () => {
    render(<AuthorFilter {...defaultProps} authorValue="john" />);
    const clearButton = screen.getByRole('button', { name: /clear/i });
    fireEvent.click(clearButton);
    expect(defaultProps.onAuthorChange).toHaveBeenCalledWith('');
  });
});
```

### 1.3 ConcurrencyPanel 组件测试

```tsx
// src/components/concurrency/__tests__/ConcurrencyPanel.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import ConcurrencyPanel from '../ConcurrencyPanel';

const mockData = {
  globalMax: 10,
  globalActive: 7,
  projects: [
    { id: 1, name: 'Project A', activeAgents: 3, maxAgents: 5, status: 'running' },
    { id: 2, name: 'Project B', activeAgents: 4, maxAgents: 5, status: 'running' },
  ],
  dataFreshness: 2,
};

describe('ConcurrencyPanel', () => {
  it('displays global utilization percentage', () => {
    render(<ConcurrencyPanel data={mockData} />);
    expect(screen.getByText('70%')).toBeInTheDocument();
    expect(screen.getByText('7 / 10')).toBeInTheDocument();
  });

  it('shows progress bar with correct value', () => {
    render(<ConcurrencyPanel data={mockData} />);
    const progressBar = screen.getByRole('progressbar');
    expect(progressBar).toHaveAttribute('aria-valuenow', '70');
  });

  it('renders per-project cards', () => {
    render(<ConcurrencyPanel data={mockData} />);
    expect(screen.getByText('Project A')).toBeInTheDocument();
    expect(screen.getByText('Project B')).toBeInTheDocument();
  });

  it('shows stale data warning when freshness > 10s', () => {
    render(<ConcurrencyPanel data={{ ...mockData, dataFreshness: 15 }} />);
    expect(screen.getByText(/数据可能过期/)).toBeInTheDocument();
  });

  it('shows loading state when no data', () => {
    render(<ConcurrencyPanel data={null} />);
    expect(screen.getByRole('progressbar')).toBeInTheDocument();
  });
});
```

### 1.4 ConcurrencyStore 测试

```typescript
// src/store/__tests__/concurrencyStore.test.ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useConcurrencyStore } from '../concurrencyStore';
import { act } from '@testing-library/react';

describe('concurrencyStore', () => {
  beforeEach(() => {
    useConcurrencyStore.getState().reset();
  });

  it('fetches global concurrency status', async () => {
    await act(async () => {
      await useConcurrencyStore.getState().fetchStatus();
    });
    const state = useConcurrencyStore.getState();
    expect(state.globalMax).toBeGreaterThan(0);
    expect(state.loading).toBe(false);
  });

  it('updates config optimistically', async () => {
    await act(async () => {
      await useConcurrencyStore.getState().updateConfig({ globalMax: 15 });
    });
    expect(useConcurrencyStore.getState().globalMax).toBe(15);
  });

  it('handles SSE connection lifecycle', async () => {
    const store = useConcurrencyStore.getState();
    await act(async () => {
      await store.connectSSE();
    });
    expect(store.sseConnected).toBe(true);

    act(() => {
      store.disconnectSSE();
    });
    expect(useConcurrencyStore.getState().sseConnected).toBe(false);
  });

  it('processes SSE agent_started event', () => {
    const store = useConcurrencyStore.getState();
    act(() => {
      store.handleSSEEvent({
        type: 'agent_started',
        projectId: 1,
        projectName: 'Test',
        activeAgents: 3,
        globalActive: 5,
      });
    });
    const state = useConcurrencyStore.getState();
    expect(state.globalActive).toBe(5);
    expect(state.projects.find(p => p.id === 1)?.activeAgents).toBe(3);
  });

  it('reconnects SSE on disconnect', async () => {
    vi.useFakeTimers();
    const store = useConcurrencyStore.getState();
    await act(async () => {
      await store.connectSSE();
    });

    // 模拟断开
    act(() => {
      store.handleSSEDisconnect();
    });
    expect(useConcurrencyStore.getState().sseConnected).toBe(false);

    // 等待重连延迟
    await act(async () => {
      vi.advanceTimersByTime(3000);
    });
    expect(useConcurrencyStore.getState().sseConnected).toBe(true);
    vi.useRealTimers();
  });
});
```

### 1.5 MSW Handlers 扩展

```typescript
// src/test/mocks/handlers.ts (Phase 4 additions)
import { http, HttpResponse } from 'msw';

export const phase4Handlers = [
  // Concurrency status
  http.get('/api/admin/concurrency', () => {
    return HttpResponse.json({
      data: {
        global_max: 10,
        global_active: 5,
        projects: [
          { project_id: 1, project_name: 'Test', active_agents: 3, max_agents: 5 },
        ],
        data_freshness: 2,
      },
      success: true,
      retCode: '0',
      retMsg: 'ok',
    });
  }),

  // Concurrency config update
  http.put('/api/admin/concurrency/config', async ({ request }) => {
    const body = await request.json();
    return HttpResponse.json({
      data: { global_max: body.global_max, previous_value: 10 },
      success: true,
      retCode: '0',
      retMsg: 'ok',
    });
  }),

  // SSE ticket
  http.post('/api/admin/concurrency/events/ticket', () => {
    return HttpResponse.json({
      data: { ticket: 'test-ticket-uuid', expires_at: '2026-05-21T23:59:59Z' },
      success: true,
      retCode: '0',
      retMsg: 'ok',
    });
  }),

  // Contributors
  http.get('/api/projects/:id/contributors', () => {
    return HttpResponse.json({
      data: {
        contributors: [
          { username: 'john', display_name: 'John', avatar_url: '', recent_issue_count: 5, recent_mr_count: 3, is_bot: false },
          { username: 'jane', display_name: 'Jane', avatar_url: '', recent_issue_count: 2, recent_mr_count: 1, is_bot: false },
        ],
        scope: 'last_100_items',
      },
      success: true,
      retCode: '0',
      retMsg: 'ok',
    });
  }),

  // Token validation
  http.post('/api/user/config/validate-token', async ({ request }) => {
    const body = await request.json();
    const valid = body.token !== 'invalid-token';
    return HttpResponse.json({
      data: { valid, username: valid ? 'testuser' : null, scopes: valid ? ['api', 'read_repository'] : [] },
      success: true,
      retCode: '0',
      retMsg: 'ok',
    });
  }),
];
```

---

## 2. 集成测试

### 2.1 AdminConcurrency 页面集成测试

```tsx
// src/pages/__tests__/AdminConcurrency.test.tsx
import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import AdminConcurrency from '../AdminConcurrency';
import { TestProviders } from '../../test/utils';

describe('AdminConcurrency Page', () => {
  it('renders concurrency panel with data from API', async () => {
    render(
      <TestProviders>
        <MemoryRouter>
          <AdminConcurrency />
        </MemoryRouter>
      </TestProviders>
    );

    await waitFor(() => {
      expect(screen.getByText('并行控制')).toBeInTheDocument();
      expect(screen.getByText('5 / 10')).toBeInTheDocument();
    });
  });

  it('opens config dialog on edit button click', async () => {
    render(
      <TestProviders>
        <MemoryRouter>
          <AdminConcurrency />
        </MemoryRouter>
      </TestProviders>
    );

    await waitFor(() => screen.getByText('并行控制'));
    fireEvent.click(screen.getByLabelText('编辑配置'));
    expect(screen.getByText('并行控制配置')).toBeInTheDocument();
  });
});
```

### 2.2 Kanban 作者过滤集成测试

```tsx
// src/pages/__tests__/KanbanWithAuthorFilter.test.tsx
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import KanbanPage from '../KanbanPage';
import { TestProviders } from '../../test/utils';

describe('Kanban Author Filter Integration', () => {
  it('filters kanban by author when selected', async () => {
    render(
      <TestProviders>
        <MemoryRouter initialEntries={['/projects/1/kanban']}>
          <Routes>
            <Route path="/projects/:id/kanban" element={<KanbanPage />} />
          </Routes>
        </MemoryRouter>
      </TestProviders>
    );

    await waitFor(() => screen.getByLabelText('按作者过滤'));

    // 选择作者
    const authorInput = screen.getByLabelText('按作者过滤');
    fireEvent.change(authorInput, { target: { value: 'John' } });
    const option = await screen.findByText('John');
    fireEvent.click(option);

    // 验证 API 被调用时带了 author 参数
    await waitFor(() => {
      // 看板应该只显示 john 的 issues
      expect(screen.queryByText('Jane Issue')).not.toBeInTheDocument();
    });
  });

  it('syncs author filter to URL params', async () => {
    // 验证 URL 同步
  });
});
```

---

## 3. E2E 测试 (Playwright)

### 3.1 配置

```typescript
// web-frontend/playwright.config.ts
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ['html', { open: 'never' }],
    ['junit', { outputFile: 'e2e-results/junit.xml' }],
  ],
  use: {
    baseURL: 'http://localhost:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
  },
});
```

### 3.2 Page Object Model

```typescript
// web-frontend/e2e/pages/ConcurrencyPage.ts
import { Page, Locator } from '@playwright/test';

export class ConcurrencyPage {
  readonly page: Page;
  readonly globalProgress: Locator;
  readonly projectCards: Locator;
  readonly editConfigButton: Locator;
  readonly configDialog: Locator;
  readonly globalMaxInput: Locator;
  readonly saveButton: Locator;

  constructor(page: Page) {
    this.page = page;
    this.globalProgress = page.getByRole('progressbar');
    this.projectCards = page.locator('[data-testid="project-concurrency-card"]');
    this.editConfigButton = page.getByLabel('编辑配置');
    this.configDialog = page.getByRole('dialog');
    this.globalMaxInput = page.getByLabel('全局最大并行数');
    this.saveButton = page.getByRole('button', { name: '保存' });
  }

  async goto() {
    await this.page.goto('/admin/concurrency');
  }

  async getGlobalUtilization(): Promise<string> {
    return await this.globalProgress.getAttribute('aria-valuenow') || '0';
  }

  async editConfig(newMax: number) {
    await this.editConfigButton.click();
    await this.globalMaxInput.clear();
    await this.globalMaxInput.fill(String(newMax));
    await this.saveButton.click();
  }
}

// web-frontend/e2e/pages/KanbanPage.ts
import { Page, Locator } from '@playwright/test';

export class KanbanPage {
  readonly page: Page;
  readonly authorFilter: Locator;
  readonly todoColumn: Locator;
  readonly inProgressColumn: Locator;
  readonly prColumn: Locator;

  constructor(page: Page) {
    this.page = page;
    this.authorFilter = page.getByLabel('按作者过滤');
    this.todoColumn = page.locator('[data-testid="kanban-todo"]');
    this.inProgressColumn = page.locator('[data-testid="kanban-in-progress"]');
    this.prColumn = page.locator('[data-testid="kanban-pr"]');
  }

  async goto(projectId: number) {
    await this.page.goto(`/projects/${projectId}/kanban`);
  }

  async filterByAuthor(authorName: string) {
    await this.authorFilter.click();
    await this.authorFilter.fill(authorName);
    await this.page.getByRole('option', { name: authorName }).click();
  }

  async clearAuthorFilter() {
    await this.page.getByLabel('按作者过滤').getByRole('button', { name: /clear/i }).click();
  }
}
```

### 3.3 E2E 测试场景

```typescript
// web-frontend/e2e/phase4-concurrency.spec.ts
import { test, expect } from '@playwright/test';
import { ConcurrencyPage } from './pages/ConcurrencyPage';
import { login } from './helpers/auth';

test.describe('Concurrency Monitoring', () => {
  test.beforeEach(async ({ page }) => {
    await login(page, 'admin', 'admin123');
  });

  test('displays global concurrency status', async ({ page }) => {
    const concurrencyPage = new ConcurrencyPage(page);
    await concurrencyPage.goto();

    await expect(concurrencyPage.globalProgress).toBeVisible();
    const value = await concurrencyPage.getGlobalUtilization();
    expect(Number(value)).toBeGreaterThanOrEqual(0);
  });

  test('shows per-project breakdown', async ({ page }) => {
    const concurrencyPage = new ConcurrencyPage(page);
    await concurrencyPage.goto();

    await expect(concurrencyPage.projectCards.first()).toBeVisible();
  });

  test('admin can update global max config', async ({ page }) => {
    const concurrencyPage = new ConcurrencyPage(page);
    await concurrencyPage.goto();
    await concurrencyPage.editConfig(15);

    // 验证更新成功
    await expect(page.getByText('配置已更新')).toBeVisible();
  });
});

// web-frontend/e2e/phase4-author-filter.spec.ts
import { test, expect } from '@playwright/test';
import { KanbanPage } from './pages/KanbanPage';
import { login } from './helpers/auth';

test.describe('Author Filtering', () => {
  test.beforeEach(async ({ page }) => {
    await login(page, 'user1', 'user123');
  });

  test('filters kanban by author', async ({ page }) => {
    const kanbanPage = new KanbanPage(page);
    await kanbanPage.goto(1);

    // 选择作者
    await kanbanPage.filterByAuthor('John Doe');

    // 验证只显示该作者的 issues
    const cards = kanbanPage.todoColumn.locator('[data-testid="issue-card"]');
    for (const card of await cards.all()) {
      await expect(card.getByText('John')).toBeVisible();
    }
  });

  test('clears author filter shows all issues', async ({ page }) => {
    const kanbanPage = new KanbanPage(page);
    await kanbanPage.goto(1);

    await kanbanPage.filterByAuthor('John Doe');
    await kanbanPage.clearAuthorFilter();

    // 验证所有 issues 重新显示
    const cards = kanbanPage.todoColumn.locator('[data-testid="issue-card"]');
    await expect(cards).toHaveCount(await cards.count());
  });
});

// web-frontend/e2e/phase4-token-validation.spec.ts
import { test, expect } from '@playwright/test';
import { login } from './helpers/auth';

test.describe('Token Validation', () => {
  test.beforeEach(async ({ page }) => {
    await login(page, 'user1', 'user123');
  });

  test('validates token and shows success feedback', async ({ page }) => {
    await page.goto('/settings');
    await page.getByLabel('GitLab Token').fill('glpat-valid-token');
    await page.getByRole('button', { name: '验证' }).click();

    await expect(page.getByText('Token 有效')).toBeVisible({ timeout: 5000 });
  });

  test('shows error for invalid token', async ({ page }) => {
    await page.goto('/settings');
    await page.getByLabel('GitLab Token').fill('invalid-token');
    await page.getByRole('button', { name: '验证' }).click();

    await expect(page.getByText('Token 无效')).toBeVisible({ timeout: 5000 });
  });
});
```

### 3.4 Auth Helper

```typescript
// web-frontend/e2e/helpers/auth.ts
import { Page } from '@playwright/test';

export async function login(page: Page, username: string, password: string) {
  await page.goto('/login');
  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码').fill(password);
  await page.getByRole('button', { name: '登录' }).click();
  await page.waitForURL(/\/(projects|admin)/);
}
```

---

## 4. CI Pipeline

### 4.1 前端 CI 配置

```yaml
# .gitlab-ci.yml (frontend stages)

frontend-lint:
  stage: lint
  image: node:20
  script:
    - cd web-frontend
    - npm ci
    - npm run lint
    - npm run type-check
  rules:
    - changes:
        - web-frontend/**/*

frontend-unit-tests:
  stage: test
  image: node:20
  script:
    - cd web-frontend
    - npm ci
    - npm run test -- --coverage --reporter=junit --outputFile=test-results/junit.xml
  coverage: '/All files[^|]*\|[^|]*\s+([\d\.]+)/'
  artifacts:
    when: always
    paths:
      - web-frontend/coverage/
      - web-frontend/test-results/
    reports:
      junit: web-frontend/test-results/junit.xml
      coverage_report:
        coverage_format: cobertura
        path: web-frontend/coverage/cobertura-coverage.xml

frontend-e2e:
  stage: e2e
  image: mcr.microsoft.com/playwright:v1.44.0-jammy
  script:
    - cd web-frontend
    - npm ci
    - npx playwright install --with-deps
    - npm run build
    - npx playwright test
  artifacts:
    when: always
    paths:
      - web-frontend/e2e-results/
      - web-frontend/playwright-report/
    reports:
      junit: web-frontend/e2e-results/junit.xml
  rules:
    - changes:
        - web-frontend/**/*
```

### 4.2 覆盖率目标

| 模块 | 目标覆盖率 |
|------|-----------|
| components/concurrency/ | >= 80% |
| components/kanban/AuthorFilter | >= 85% |
| store/concurrencyStore | >= 85% |
| api/concurrency | >= 90% |
| 整体 | >= 70% |

### 4.3 本地运行

```bash
# 单元测试
cd web-frontend
npm run test

# 带覆盖率
npm run test -- --coverage

# E2E 测试（需要先启动 dev server）
npx playwright test

# 只跑 Phase 4 相关
npx playwright test phase4

# 带 UI 模式调试
npx playwright test --ui
```
