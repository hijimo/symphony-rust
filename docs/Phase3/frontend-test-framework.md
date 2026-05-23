# Phase 3 前端测试自动化框架设计（看板/Kanban + Issue 创建）

## 1. 测试目录结构

```
web-frontend/
├── vitest.config.ts                          # 单元/集成测试配置
├── playwright.config.ts                      # E2E 测试配置
├── src/
│   ├── test/
│   │   ├── setup.ts                          # Vitest 全局 setup（已有）
│   │   ├── utils.tsx                         # 自定义 render（扩展）
│   │   ├── sse-utils.ts                      # SSE 流式 mock 工具（新增）
│   │   ├── fixtures/
│   │   │   ├── projects.ts                   # 项目 mock 数据（已有）
│   │   │   ├── kanban.ts                     # 看板 mock 数据（新增）
│   │   │   ├── issues.ts                     # Issue mock 数据（新增）
│   │   │   └── pull-requests.ts              # PR mock 数据（新增）
│   │   └── mocks/
│   │       ├── handlers.ts                   # Phase 1 handlers（已有）
│   │       ├── handlers-kanban.ts            # 看板 API handlers（新增）
│   │       ├── handlers-issues.ts            # Issue API handlers（新增）
│   │       ├── handlers-ai-generate.ts       # AI 生成 SSE handlers（新增）
│   │       └── server.ts                     # MSW server（扩展）
│   ├── api/__tests__/
│   │   ├── client.test.ts                    # API 客户端测试（已有）
│   │   ├── kanban.test.ts                    # 看板 API 层测试（新增）
│   │   └── issues.test.ts                    # Issue API 层测试（新增）
│   ├── store/__tests__/
│   │   ├── auth.test.ts                      # Auth Store 测试（已有）
│   │   ├── projectStore.test.ts              # 项目 Store 测试（已有）
│   │   ├── kanbanStore.test.ts               # 看板 Store 测试（新增）
│   │   └── issueStore.test.ts                # Issue Store 测试（新增）
│   ├── components/kanban/__tests__/
│   │   ├── KanbanBoard.test.tsx              # 看板主组件测试
│   │   ├── KanbanColumn.test.tsx             # 看板列组件测试
│   │   ├── IssueCard.test.tsx                # Issue 卡片测试
│   │   ├── PrCard.test.tsx                   # PR 卡片测试
│   │   ├── AuthorFilter.test.tsx             # 作者筛选器测试
│   │   └── StatusBadge.test.tsx              # 状态徽章测试
│   ├── components/issues/__tests__/
│   │   ├── IssueForm.test.tsx                # Issue 表单测试
│   │   ├── AiGenerateButton.test.tsx         # AI 生成按钮测试
│   │   ├── StreamingDisplay.test.tsx         # 流式展示组件测试
│   │   ├── MarkdownEditor.test.tsx           # Markdown 编辑器测试
│   │   └── LabelSelector.test.tsx            # 标签选择器测试
│   └── pages/__tests__/
│       ├── KanbanPage.test.tsx               # 看板页面集成测试
│       └── CreateIssuePage.test.tsx           # 创建 Issue 页面集成测试
├── e2e/
│   ├── auth.setup.ts                         # 认证 setup（已有）
│   ├── global-setup.ts                       # 全局 setup（已有）
│   ├── page-objects/
│   │   ├── KanbanPage.ts                     # 看板页面对象（新增）
│   │   └── CreateIssuePage.ts                # 创建 Issue 页面对象（新增）
│   ├── kanban.spec.ts                        # 看板 E2E 测试（新增）
│   ├── issue-creation.spec.ts                # Issue 创建 E2E 测试（新增）
│   ├── ai-generation.spec.ts                 # AI 生成 E2E 测试（新增）
│   ├── kanban-responsive.spec.ts             # 响应式测试（新增）
│   └── kanban-accessibility.spec.ts          # 无障碍测试（新增）
└── .gitlab-ci.yml                            # CI 配置
```

---

## 2. 单元测试（Vitest + React Testing Library）

### 2.1 组件单元测试策略

遵循已有项目约定：
- 测试文件放在 `__tests__/` 目录下，命名为 `*.test.tsx`
- 使用 `renderWithProviders` 或 `renderAsAdmin` 包装组件
- 使用 `userEvent` 模拟用户交互
- 使用 `waitFor` 处理异步状态更新
- MSW 拦截 API 请求，`server.use()` 覆盖特殊场景

### 2.2 Hook 测试

Phase 3 新增的自定义 Hooks 需要独立测试：

```typescript
// src/hooks/__tests__/useSSE.test.ts
import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useSSE } from '../useSSE';

describe('useSSE', () => {
  let mockEventSource: any;

  beforeEach(() => {
    mockEventSource = {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      close: vi.fn(),
      readyState: 0,
    };
    vi.stubGlobal('EventSource', vi.fn(() => mockEventSource));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('initializes with idle status', () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));
    expect(result.current.status).toBe('idle');
    expect(result.current.data).toBe('');
  });

  it('transitions to streaming on start', () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    expect(result.current.status).toBe('streaming');
  });

  it('accumulates chunks during streaming', async () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    // Simulate message events
    const onMessage = mockEventSource.addEventListener.mock.calls.find(
      ([event]: [string]) => event === 'message'
    )?.[1];

    act(() => {
      onMessage({ data: JSON.stringify({ type: 'chunk', content: 'Hello ' }) });
      onMessage({ data: JSON.stringify({ type: 'chunk', content: 'World' }) });
    });

    expect(result.current.data).toBe('Hello World');
  });

  it('transitions to done on completion', async () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    const onMessage = mockEventSource.addEventListener.mock.calls.find(
      ([event]: [string]) => event === 'message'
    )?.[1];

    act(() => {
      onMessage({ data: JSON.stringify({ type: 'done', content: 'Full content' }) });
    });

    expect(result.current.status).toBe('done');
    expect(result.current.data).toBe('Full content');
  });

  it('handles error events', async () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    const onError = mockEventSource.addEventListener.mock.calls.find(
      ([event]: [string]) => event === 'error'
    )?.[1];

    act(() => {
      onError(new Event('error'));
    });

    expect(result.current.status).toBe('error');
  });

  it('closes connection on abort', () => {
    const { result } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    act(() => {
      result.current.abort();
    });

    expect(mockEventSource.close).toHaveBeenCalled();
    expect(result.current.status).toBe('idle');
  });

  it('cleans up on unmount', () => {
    const { result, unmount } = renderHook(() => useSSE('/api/projects/1/issues/ai-generate'));

    act(() => {
      result.current.start({ prompt: 'test' });
    });

    unmount();
    expect(mockEventSource.close).toHaveBeenCalled();
  });
});
```

```typescript
// src/hooks/__tests__/useKanbanFilter.test.ts
import { renderHook, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { useKanbanFilter } from '../useKanbanFilter';

describe('useKanbanFilter', () => {
  const mockIssues = [
    { id: 1, title: 'Bug fix', author: { username: 'alice' }, labels: ['bug'] },
    { id: 2, title: 'Feature', author: { username: 'bob' }, labels: ['feature'] },
    { id: 3, title: 'Docs', author: { username: 'alice' }, labels: ['docs'] },
  ];

  it('returns all issues when no filter applied', () => {
    const { result } = renderHook(() => useKanbanFilter(mockIssues));
    expect(result.current.filtered).toHaveLength(3);
  });

  it('filters by author', () => {
    const { result } = renderHook(() => useKanbanFilter(mockIssues));

    act(() => {
      result.current.setAuthorFilter('alice');
    });

    expect(result.current.filtered).toHaveLength(2);
    expect(result.current.filtered.every((i) => i.author.username === 'alice')).toBe(true);
  });

  it('clears filter', () => {
    const { result } = renderHook(() => useKanbanFilter(mockIssues));

    act(() => {
      result.current.setAuthorFilter('alice');
    });
    expect(result.current.filtered).toHaveLength(2);

    act(() => {
      result.current.clearFilter();
    });
    expect(result.current.filtered).toHaveLength(3);
  });

  it('extracts unique authors', () => {
    const { result } = renderHook(() => useKanbanFilter(mockIssues));
    expect(result.current.authors).toEqual(['alice', 'bob']);
  });
});
```

### 2.3 Store 测试（Zustand）

```typescript
// src/store/__tests__/kanbanStore.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { useKanbanStore } from '../kanbanStore';

describe('useKanbanStore', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    useKanbanStore.setState({
      pending: [],
      inProgress: [],
      pullRequests: [],
      loading: false,
      error: null,
      authorFilter: null,
    });
  });

  it('has correct initial state', () => {
    const state = useKanbanStore.getState();
    expect(state.pending).toEqual([]);
    expect(state.inProgress).toEqual([]);
    expect(state.pullRequests).toEqual([]);
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it('fetchKanbanData populates all columns', async () => {
    await useKanbanStore.getState().fetchKanbanData(1);

    const state = useKanbanStore.getState();
    expect(state.pending.length).toBeGreaterThan(0);
    expect(state.inProgress.length).toBeGreaterThan(0);
    expect(state.pullRequests.length).toBeGreaterThan(0);
    expect(state.loading).toBe(false);
  });

  it('fetchKanbanData sets loading during request', async () => {
    const promise = useKanbanStore.getState().fetchKanbanData(1);
    expect(useKanbanStore.getState().loading).toBe(true);
    await promise;
    expect(useKanbanStore.getState().loading).toBe(false);
  });

  it('fetchKanbanData handles API error', async () => {
    localStorage.removeItem('token');

    await useKanbanStore.getState().fetchKanbanData(1).catch(() => {});

    const state = useKanbanStore.getState();
    expect(state.loading).toBe(false);
    expect(state.error).not.toBeNull();
  });

  it('setAuthorFilter updates filter state', () => {
    useKanbanStore.getState().setAuthorFilter('alice');
    expect(useKanbanStore.getState().authorFilter).toBe('alice');
  });

  it('clearAuthorFilter resets filter', () => {
    useKanbanStore.getState().setAuthorFilter('alice');
    useKanbanStore.getState().clearAuthorFilter();
    expect(useKanbanStore.getState().authorFilter).toBeNull();
  });

  it('refresh re-fetches kanban data', async () => {
    await useKanbanStore.getState().fetchKanbanData(1);
    const firstFetch = useKanbanStore.getState().pending;

    await useKanbanStore.getState().refresh(1);
    const secondFetch = useKanbanStore.getState().pending;

    // Data should be refreshed (same mock data in test)
    expect(secondFetch).toEqual(firstFetch);
  });
});
```

```typescript
// src/store/__tests__/issueStore.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { useIssueStore } from '../issueStore';

describe('useIssueStore', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    useIssueStore.setState({
      creating: false,
      aiGenerating: false,
      aiContent: '',
      error: null,
    });
  });

  it('has correct initial state', () => {
    const state = useIssueStore.getState();
    expect(state.creating).toBe(false);
    expect(state.aiGenerating).toBe(false);
    expect(state.aiContent).toBe('');
  });

  it('createIssue sets creating flag during request', async () => {
    const promise = useIssueStore.getState().createIssue(1, {
      title: 'Test Issue',
      description: '## Description\n\nTest',
      labels: ['bug'],
    });

    expect(useIssueStore.getState().creating).toBe(true);
    await promise;
    expect(useIssueStore.getState().creating).toBe(false);
  });

  it('createIssue returns created issue data', async () => {
    const result = await useIssueStore.getState().createIssue(1, {
      title: 'Test Issue',
      description: '## Description\n\nTest',
      labels: ['bug'],
    });

    expect(result.title).toBe('Test Issue');
    expect(result.iid).toBeDefined();
  });

  it('createIssue handles validation error', async () => {
    await expect(
      useIssueStore.getState().createIssue(1, {
        title: '',
        description: '',
        labels: [],
      }),
    ).rejects.toThrow();

    expect(useIssueStore.getState().creating).toBe(false);
  });

  it('setAiContent updates generated content', () => {
    useIssueStore.getState().setAiContent('## Description\n\nGenerated');
    expect(useIssueStore.getState().aiContent).toBe('## Description\n\nGenerated');
  });

  it('resetAiContent clears generated content', () => {
    useIssueStore.getState().setAiContent('some content');
    useIssueStore.getState().resetAiContent();
    expect(useIssueStore.getState().aiContent).toBe('');
  });
});
```

### 2.4 工具函数测试

```typescript
// src/utils/__tests__/kanbanHelpers.test.ts
import { describe, it, expect } from 'vitest';
import {
  categorizeIssues,
  formatIssueDate,
  extractAuthors,
  getPrStatusColor,
  getPrStatusText,
} from '../kanbanHelpers';

describe('categorizeIssues', () => {
  const issues = [
    { id: 1, labels: ['bug'], state: 'opened' },
    { id: 2, labels: ['symphony-claimed', 'feature'], state: 'opened' },
    { id: 3, labels: ['enhancement'], state: 'opened' },
  ];

  it('separates pending from in-progress by symphony-claimed label', () => {
    const { pending, inProgress } = categorizeIssues(issues);
    expect(pending).toHaveLength(2);
    expect(inProgress).toHaveLength(1);
    expect(inProgress[0].id).toBe(2);
  });

  it('returns empty arrays for empty input', () => {
    const { pending, inProgress } = categorizeIssues([]);
    expect(pending).toEqual([]);
    expect(inProgress).toEqual([]);
  });
});

describe('formatIssueDate', () => {
  it('formats ISO date to relative time', () => {
    const now = new Date();
    const oneHourAgo = new Date(now.getTime() - 3600000).toISOString();
    expect(formatIssueDate(oneHourAgo)).toMatch(/1.*小时前/);
  });

  it('formats date older than 7 days as absolute date', () => {
    const oldDate = '2024-01-01T00:00:00Z';
    expect(formatIssueDate(oldDate)).toMatch(/2024/);
  });

  it('handles invalid date gracefully', () => {
    expect(formatIssueDate('')).toBe('-');
    expect(formatIssueDate(null as any)).toBe('-');
  });
});

describe('extractAuthors', () => {
  it('extracts unique authors from issues', () => {
    const issues = [
      { author: { username: 'alice', avatar_url: 'a.png' } },
      { author: { username: 'bob', avatar_url: 'b.png' } },
      { author: { username: 'alice', avatar_url: 'a.png' } },
    ];
    const authors = extractAuthors(issues);
    expect(authors).toHaveLength(2);
    expect(authors.map((a) => a.username)).toEqual(['alice', 'bob']);
  });
});

describe('getPrStatusColor', () => {
  it('returns success for merged', () => {
    expect(getPrStatusColor('merged')).toBe('success');
  });

  it('returns info for opened', () => {
    expect(getPrStatusColor('opened')).toBe('info');
  });

  it('returns error for closed', () => {
    expect(getPrStatusColor('closed')).toBe('error');
  });

  it('returns default for unknown status', () => {
    expect(getPrStatusColor('unknown')).toBe('default');
  });
});

describe('getPrStatusText', () => {
  it('returns Chinese text for each status', () => {
    expect(getPrStatusText('merged')).toBe('已合并');
    expect(getPrStatusText('opened')).toBe('进行中');
    expect(getPrStatusText('closed')).toBe('已关闭');
  });
});
```

### 2.5 覆盖率目标

| 层级 | 目标覆盖率 | 说明 |
|------|-----------|------|
| 组件单元测试 | >= 80% | 所有新组件的 branches/statements |
| Store 测试 | >= 90% | 所有 actions 和 selectors |
| Hook 测试 | >= 90% | 自定义 Hooks 全路径覆盖 |
| 工具函数 | >= 95% | 纯函数全覆盖 |
| 页面集成测试 | >= 70% | 主要用户路径 |
| E2E 测试 | 核心流程 100% | 看板浏览/Issue 创建/AI 生成 |


---

## 3. 集成测试

### 3.1 页面级集成测试

#### KanbanPage (`pages/__tests__/KanbanPage.test.tsx`)

```typescript
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../test/mocks/server';
import { renderAsAdmin } from '../../test/utils';
import KanbanPage from '../KanbanPage';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return { ...actual, useNavigate: () => mockNavigate, useParams: () => ({ projectId: '1' }) };
});

describe('KanbanPage', () => {
  it('renders three-column layout after loading', async () => {
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => {
      expect(screen.getByTestId('column-pending')).toBeInTheDocument();
      expect(screen.getByTestId('column-in-progress')).toBeInTheDocument();
      expect(screen.getByTestId('column-pull-requests')).toBeInTheDocument();
    });
  });

  it('shows loading skeleton during data fetch', () => {
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });
    expect(screen.getAllByTestId('skeleton-card').length).toBeGreaterThan(0);
  });

  it('displays issue cards in pending column', async () => {
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => {
      const pendingColumn = screen.getByTestId('column-pending');
      expect(within(pendingColumn).getAllByTestId('issue-card').length).toBeGreaterThan(0);
    });
  });

  it('displays PR cards with status badges', async () => {
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => {
      const prColumn = screen.getByTestId('column-pull-requests');
      expect(within(prColumn).getAllByTestId('pr-card').length).toBeGreaterThan(0);
    });
  });

  it('shows empty state when no issues exist', async () => {
    server.use(
      http.get('*/api/projects/:id/kanban', () =>
        HttpResponse.json({
          success: true,
          retCode: 'SUCCESS',
          retMsg: 'ok',
          data: { pending: [], in_progress: [], pull_requests: [] },
        }),
      ),
    );

    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => {
      expect(screen.getByText(/暂无待处理的 Issue/i)).toBeInTheDocument();
    });
  });

  it('shows error state when API unavailable', async () => {
    server.use(
      http.get('*/api/projects/:id/kanban', () =>
        HttpResponse.json(
          { success: false, retCode: 'PLATFORM_ERROR', retMsg: 'GitLab API 不可达', data: null },
          { status: 502 },
        ),
      ),
    );

    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => {
      expect(screen.getByText(/GitLab API 不可达/i)).toBeInTheDocument();
    });
  });

  it('filters issues by author', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => screen.getByTestId('author-filter'));

    await user.click(screen.getByTestId('author-filter'));
    await user.click(screen.getByRole('option', { name: /alice/i }));

    await waitFor(() => {
      const pendingColumn = screen.getByTestId('column-pending');
      const cards = within(pendingColumn).getAllByTestId('issue-card');
      cards.forEach((card) => {
        expect(within(card).getByText(/alice/i)).toBeInTheDocument();
      });
    });
  });

  it('refreshes data on refresh button click', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => screen.getByTestId('column-pending'));

    await user.click(screen.getByRole('button', { name: /刷新/i }));

    // Should show loading briefly then re-render
    await waitFor(() => {
      expect(screen.getByTestId('column-pending')).toBeInTheDocument();
    });
  });

  it('navigates to create issue page', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<KanbanPage />, { initialEntries: ['/projects/1/kanban'] });

    await waitFor(() => screen.getByRole('button', { name: /创建 Issue/i }));
    await user.click(screen.getByRole('button', { name: /创建 Issue/i }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects/1/issues/new');
  });
});
```

#### CreateIssuePage (`pages/__tests__/CreateIssuePage.test.tsx`)

```typescript
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../test/mocks/server';
import { renderAsAdmin } from '../../test/utils';
import CreateIssuePage from '../CreateIssuePage';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return { ...actual, useNavigate: () => mockNavigate, useParams: () => ({ projectId: '1' }) };
});

describe('CreateIssuePage', () => {
  it('renders form with all required fields', () => {
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    expect(screen.getByLabelText(/标题/i)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/用一句话描述/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /AI 生成/i })).toBeInTheDocument();
    expect(screen.getByTestId('markdown-editor')).toBeInTheDocument();
    expect(screen.getByTestId('label-selector')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /创建 Issue/i })).toBeInTheDocument();
  });

  it('validates title is required on submit', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.click(screen.getByRole('button', { name: /创建 Issue/i }));

    await waitFor(() => {
      expect(screen.getByText(/标题不能为空/i)).toBeInTheDocument();
    });
  });

  it('submits form and navigates to kanban', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.type(screen.getByLabelText(/标题/i), 'Fix login bug');
    // Fill markdown editor
    const editor = screen.getByTestId('markdown-editor');
    await user.type(editor, '## Description\n\nFix the login page');

    await user.click(screen.getByRole('button', { name: /创建 Issue/i }));

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalledWith('/projects/1/kanban');
    });
  });

  it('shows error on API failure', async () => {
    server.use(
      http.post('*/api/projects/:id/issues', () =>
        HttpResponse.json(
          { success: false, retCode: 'PLATFORM_ERROR', retMsg: '创建失败', data: null },
          { status: 500 },
        ),
      ),
    );

    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.type(screen.getByLabelText(/标题/i), 'Test Issue');
    await user.click(screen.getByRole('button', { name: /创建 Issue/i }));

    await waitFor(() => {
      expect(screen.getByText(/创建失败/i)).toBeInTheDocument();
    });
  });

  it('AI generate button triggers streaming', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.type(screen.getByPlaceholderText(/用一句话描述/i), '修复登录页面样式');
    await user.click(screen.getByRole('button', { name: /AI 生成/i }));

    await waitFor(() => {
      expect(screen.getByTestId('streaming-indicator')).toBeInTheDocument();
    });
  });

  it('cancel button navigates back', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.click(screen.getByRole('button', { name: /取消/i }));
    expect(mockNavigate).toHaveBeenCalledWith(-1);
  });

  it('label selector allows multiple selection', async () => {
    const user = userEvent.setup();
    renderAsAdmin(<CreateIssuePage />, { initialEntries: ['/projects/1/issues/new'] });

    await user.click(screen.getByTestId('label-selector'));
    await user.click(screen.getByRole('option', { name: /bug/i }));
    await user.click(screen.getByRole('option', { name: /feature/i }));

    expect(screen.getByText('bug')).toBeInTheDocument();
    expect(screen.getByText('feature')).toBeInTheDocument();
  });
});
```

### 3.2 API 集成测试（MSW）

```typescript
// src/api/__tests__/kanban.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { fetchKanbanData, refreshKanban } from '../kanban';

describe('Kanban API', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
  });

  it('fetches kanban data with three columns', async () => {
    const data = await fetchKanbanData(1);
    expect(data.pending).toBeDefined();
    expect(data.in_progress).toBeDefined();
    expect(data.pull_requests).toBeDefined();
  });

  it('includes author info in issue data', async () => {
    const data = await fetchKanbanData(1);
    expect(data.pending[0].author).toBeDefined();
    expect(data.pending[0].author.username).toBeDefined();
  });

  it('includes PR status and linked issues', async () => {
    const data = await fetchKanbanData(1);
    expect(data.pull_requests[0].state).toBeDefined();
    expect(data.pull_requests[0].related_issues).toBeDefined();
  });

  it('throws on unauthorized request', async () => {
    localStorage.removeItem('token');
    await expect(fetchKanbanData(1)).rejects.toThrow();
  });

  it('supports author filter parameter', async () => {
    const data = await fetchKanbanData(1, { author: 'alice' });
    data.pending.forEach((issue) => {
      expect(issue.author.username).toBe('alice');
    });
  });
});
```

```typescript
// src/api/__tests__/issues.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { createIssue, fetchLabels } from '../issues';

describe('Issues API', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
  });

  it('creates issue with title and description', async () => {
    const result = await createIssue(1, {
      title: 'Test Issue',
      description: '## Description\n\nTest content',
      labels: ['bug'],
    });

    expect(result.iid).toBeDefined();
    expect(result.title).toBe('Test Issue');
    expect(result.web_url).toBeDefined();
  });

  it('rejects empty title', async () => {
    await expect(
      createIssue(1, { title: '', description: 'content', labels: [] }),
    ).rejects.toThrow();
  });

  it('fetches available labels for project', async () => {
    const labels = await fetchLabels(1);
    expect(Array.isArray(labels)).toBe(true);
    expect(labels[0]).toHaveProperty('name');
    expect(labels[0]).toHaveProperty('color');
  });
});
```

### 3.3 Router 集成测试

```typescript
// src/__tests__/routing.test.tsx
import { screen, waitFor } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { renderWithProviders } from '../test/utils';
import App from '../App';

describe('Phase 3 Routing', () => {
  it('navigates to kanban page at /projects/:id/kanban', async () => {
    renderWithProviders(<App />, {
      initialEntries: ['/projects/1/kanban'],
      authState: { token: 'mock-token', isAuthenticated: true },
    });

    await waitFor(() => {
      expect(screen.getByTestId('kanban-page')).toBeInTheDocument();
    });
  });

  it('navigates to create issue page at /projects/:id/issues/new', async () => {
    renderWithProviders(<App />, {
      initialEntries: ['/projects/1/issues/new'],
      authState: { token: 'mock-token', isAuthenticated: true },
    });

    await waitFor(() => {
      expect(screen.getByTestId('create-issue-page')).toBeInTheDocument();
    });
  });

  it('redirects unauthenticated users from kanban to login', async () => {
    renderWithProviders(<App />, {
      initialEntries: ['/projects/1/kanban'],
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /登录/i })).toBeInTheDocument();
    });
  });
});
```


---

## 4. E2E 测试（Playwright）

### 4.1 Page Objects

#### `e2e/page-objects/KanbanPage.ts`

```typescript
import { Page, Locator, expect } from '@playwright/test';

export class KanbanPage {
  readonly page: Page;
  readonly pendingColumn: Locator;
  readonly inProgressColumn: Locator;
  readonly prColumn: Locator;
  readonly refreshButton: Locator;
  readonly createIssueButton: Locator;
  readonly authorFilter: Locator;
  readonly issueCards: Locator;
  readonly prCards: Locator;
  readonly loadingSkeleton: Locator;
  readonly errorMessage: Locator;
  readonly emptyState: Locator;

  constructor(page: Page) {
    this.page = page;
    this.pendingColumn = page.getByTestId('column-pending');
    this.inProgressColumn = page.getByTestId('column-in-progress');
    this.prColumn = page.getByTestId('column-pull-requests');
    this.refreshButton = page.getByRole('button', { name: /刷新/i });
    this.createIssueButton = page.getByRole('button', { name: /创建 Issue/i });
    this.authorFilter = page.getByTestId('author-filter');
    this.issueCards = page.locator('[data-testid="issue-card"]');
    this.prCards = page.locator('[data-testid="pr-card"]');
    this.loadingSkeleton = page.locator('[data-testid="skeleton-card"]');
    this.errorMessage = page.getByTestId('error-message');
    this.emptyState = page.getByTestId('empty-state');
  }

  async goto(projectId: number) {
    await this.page.goto(`/projects/${projectId}/kanban`);
    await this.page.waitForLoadState('networkidle');
  }

  async waitForDataLoad() {
    await expect(this.loadingSkeleton.first()).not.toBeVisible({ timeout: 10000 });
  }

  async getIssueCount(column: 'pending' | 'in-progress' | 'pr'): Promise<number> {
    const col = column === 'pending'
      ? this.pendingColumn
      : column === 'in-progress'
        ? this.inProgressColumn
        : this.prColumn;
    const cards = col.locator('[data-testid="issue-card"], [data-testid="pr-card"]');
    return cards.count();
  }

  async filterByAuthor(username: string) {
    await this.authorFilter.click();
    await this.page.getByRole('option', { name: new RegExp(username, 'i') }).click();
  }

  async clearAuthorFilter() {
    await this.authorFilter.click();
    await this.page.getByRole('option', { name: /全部/i }).click();
  }

  async refresh() {
    await this.refreshButton.click();
    await this.waitForDataLoad();
  }

  async clickCreateIssue() {
    await this.createIssueButton.click();
  }

  async getColumnHeader(column: 'pending' | 'in-progress' | 'pr'): Promise<string> {
    const col = column === 'pending'
      ? this.pendingColumn
      : column === 'in-progress'
        ? this.inProgressColumn
        : this.prColumn;
    const header = col.locator('h6, h5').first();
    return (await header.textContent()) ?? '';
  }
}
```

#### `e2e/page-objects/CreateIssuePage.ts`

```typescript
import { Page, Locator, expect } from '@playwright/test';

export class CreateIssuePage {
  readonly page: Page;
  readonly titleInput: Locator;
  readonly promptInput: Locator;
  readonly aiGenerateButton: Locator;
  readonly markdownEditor: Locator;
  readonly labelSelector: Locator;
  readonly submitButton: Locator;
  readonly cancelButton: Locator;
  readonly streamingIndicator: Locator;
  readonly streamingContent: Locator;
  readonly abortButton: Locator;

  constructor(page: Page) {
    this.page = page;
    this.titleInput = page.getByLabel(/标题/i);
    this.promptInput = page.getByPlaceholder(/用一句话描述/i);
    this.aiGenerateButton = page.getByRole('button', { name: /AI 生成/i });
    this.markdownEditor = page.getByTestId('markdown-editor');
    this.labelSelector = page.getByTestId('label-selector');
    this.submitButton = page.getByRole('button', { name: /创建 Issue/i });
    this.cancelButton = page.getByRole('button', { name: /取消/i });
    this.streamingIndicator = page.getByTestId('streaming-indicator');
    this.streamingContent = page.getByTestId('streaming-content');
    this.abortButton = page.getByRole('button', { name: /停止生成/i });
  }

  async goto(projectId: number) {
    await this.page.goto(`/projects/${projectId}/issues/new`);
    await this.page.waitForLoadState('networkidle');
  }

  async fillTitle(title: string) {
    await this.titleInput.fill(title);
  }

  async fillPrompt(prompt: string) {
    await this.promptInput.fill(prompt);
  }

  async triggerAiGenerate() {
    await this.aiGenerateButton.click();
  }

  async waitForStreamingComplete() {
    await expect(this.streamingIndicator).not.toBeVisible({ timeout: 30000 });
  }

  async selectLabel(label: string) {
    await this.labelSelector.click();
    await this.page.getByRole('option', { name: new RegExp(label, 'i') }).click();
    // Close dropdown
    await this.page.keyboard.press('Escape');
  }

  async fillMarkdownContent(content: string) {
    await this.markdownEditor.click();
    await this.markdownEditor.fill(content);
  }

  async submit() {
    await this.submitButton.click();
  }

  async cancel() {
    await this.cancelButton.click();
  }

  async abortGeneration() {
    await this.abortButton.click();
  }
}
```

### 4.2 E2E 测试用例

#### `e2e/kanban.spec.ts` - 看板交互

```typescript
import { test, expect } from '@playwright/test';
import { KanbanPage } from './page-objects/KanbanPage';

test.describe('Kanban Board', () => {
  let kanban: KanbanPage;

  test.beforeEach(async ({ page }) => {
    kanban = new KanbanPage(page);
  });

  test('displays three-column layout with data', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Verify three columns exist
    await expect(kanban.pendingColumn).toBeVisible();
    await expect(kanban.inProgressColumn).toBeVisible();
    await expect(kanban.prColumn).toBeVisible();

    // Verify column headers show counts
    const pendingHeader = await kanban.getColumnHeader('pending');
    expect(pendingHeader).toMatch(/待处理/);

    const inProgressHeader = await kanban.getColumnHeader('in-progress');
    expect(inProgressHeader).toMatch(/处理中/);

    const prHeader = await kanban.getColumnHeader('pr');
    expect(prHeader).toMatch(/PR/i);
  });

  test('issue cards display title, author, and labels', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const firstCard = kanban.issueCards.first();
    await expect(firstCard).toBeVisible();

    // Card should have title
    await expect(firstCard.locator('[data-testid="issue-title"]')).not.toBeEmpty();
    // Card should have author avatar or name
    await expect(firstCard.locator('[data-testid="issue-author"]')).toBeVisible();
  });

  test('PR cards display status badge', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const firstPr = kanban.prCards.first();
    await expect(firstPr).toBeVisible();

    // PR card should have status badge
    await expect(firstPr.locator('[data-testid="pr-status"]')).toBeVisible();
    // PR card should show linked issue count
    await expect(firstPr.locator('[data-testid="linked-issues"]')).toBeVisible();
  });

  test('author filter narrows displayed issues', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const totalBefore = await kanban.issueCards.count();

    await kanban.filterByAuthor('alice');

    // Wait for filter to apply
    await page.waitForTimeout(500);

    const totalAfter = await kanban.issueCards.count();
    expect(totalAfter).toBeLessThanOrEqual(totalBefore);

    // All visible cards should be by alice
    const cards = await kanban.issueCards.all();
    for (const card of cards) {
      await expect(card.locator('[data-testid="issue-author"]')).toContainText('alice');
    }
  });

  test('clear filter restores all issues', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const totalBefore = await kanban.issueCards.count();

    await kanban.filterByAuthor('alice');
    await page.waitForTimeout(500);

    await kanban.clearAuthorFilter();
    await page.waitForTimeout(500);

    const totalAfter = await kanban.issueCards.count();
    expect(totalAfter).toBe(totalBefore);
  });

  test('refresh button reloads kanban data', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Click refresh
    await kanban.refresh();

    // Verify data is still displayed (no error)
    await expect(kanban.pendingColumn).toBeVisible();
    const count = await kanban.getIssueCount('pending');
    expect(count).toBeGreaterThan(0);
  });

  test('create issue button navigates to creation page', async ({ page }) => {
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    await kanban.clickCreateIssue();
    await expect(page).toHaveURL(/\/projects\/1\/issues\/new/);
  });

  test('shows loading state initially', async ({ page }) => {
    // Intercept and delay the API response
    await page.route('**/api/projects/*/kanban', async (route) => {
      await new Promise((r) => setTimeout(r, 2000));
      await route.continue();
    });

    await page.goto('/projects/1/kanban');

    // Should show skeleton loading
    await expect(kanban.loadingSkeleton.first()).toBeVisible();
  });
});
```

#### `e2e/issue-creation.spec.ts` - Issue 创建流程

```typescript
import { test, expect } from '@playwright/test';
import { CreateIssuePage } from './page-objects/CreateIssuePage';
import { KanbanPage } from './page-objects/KanbanPage';

test.describe('Issue Creation', () => {
  let createPage: CreateIssuePage;

  test.beforeEach(async ({ page }) => {
    createPage = new CreateIssuePage(page);
  });

  test('full manual issue creation flow', async ({ page }) => {
    await createPage.goto(1);

    // Fill title
    await createPage.fillTitle('Fix mobile responsive layout');

    // Fill markdown content
    await createPage.fillMarkdownContent(
      '## Description\n\nThe login page breaks on mobile.\n\n## Acceptance Criteria\n\n- [ ] Page renders correctly on 375px width',
    );

    // Select labels
    await createPage.selectLabel('bug');

    // Submit
    await createPage.submit();

    // Should navigate back to kanban
    await expect(page).toHaveURL(/\/projects\/1\/kanban/, { timeout: 10000 });
  });

  test('form validation prevents empty title submission', async ({ page }) => {
    await createPage.goto(1);

    // Try to submit without title
    await createPage.submit();

    // Should show validation error
    await expect(page.getByText(/标题不能为空/i)).toBeVisible();

    // Should stay on the same page
    await expect(page).toHaveURL(/\/issues\/new/);
  });

  test('cancel returns to previous page', async ({ page }) => {
    // Start from kanban
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Navigate to create issue
    await kanban.clickCreateIssue();
    await expect(page).toHaveURL(/\/issues\/new/);

    // Cancel
    await createPage.cancel();

    // Should go back
    await expect(page).toHaveURL(/\/projects\/1\/kanban/);
  });

  test('label selection persists in form', async ({ page }) => {
    await createPage.goto(1);

    await createPage.selectLabel('bug');
    await createPage.selectLabel('enhancement');

    // Both labels should be visible as chips
    await expect(page.getByText('bug')).toBeVisible();
    await expect(page.getByText('enhancement')).toBeVisible();
  });
});
```

#### `e2e/ai-generation.spec.ts` - AI 生成流式展示

```typescript
import { test, expect } from '@playwright/test';
import { CreateIssuePage } from './page-objects/CreateIssuePage';

test.describe('AI Issue Generation', () => {
  let createPage: CreateIssuePage;

  test.beforeEach(async ({ page }) => {
    createPage = new CreateIssuePage(page);
    await createPage.goto(1);
  });

  test('AI generate button requires prompt input', async ({ page }) => {
    // Click AI generate without prompt
    await createPage.triggerAiGenerate();

    // Should show hint to enter prompt
    await expect(page.getByText(/请先输入需求描述/i)).toBeVisible();
  });

  test('AI generation shows streaming indicator', async ({ page }) => {
    await createPage.fillPrompt('修复登录页面在移动端的样式错乱');
    await createPage.triggerAiGenerate();

    // Should show streaming indicator
    await expect(createPage.streamingIndicator).toBeVisible({ timeout: 5000 });
  });

  test('AI generation streams content progressively', async ({ page }) => {
    await createPage.fillPrompt('添加用户头像上传功能');
    await createPage.triggerAiGenerate();

    // Wait for streaming to start
    await expect(createPage.streamingIndicator).toBeVisible({ timeout: 5000 });

    // Content should appear progressively
    await expect(createPage.streamingContent).not.toBeEmpty({ timeout: 10000 });

    // Wait for completion
    await createPage.waitForStreamingComplete();

    // Final content should contain expected structure
    const content = await createPage.markdownEditor.textContent();
    expect(content).toContain('描述');
    expect(content).toContain('Acceptance Criteria');
    expect(content).toContain('Validation');
  });

  test('AI generation can be aborted', async ({ page }) => {
    await createPage.fillPrompt('重构数据库查询层');
    await createPage.triggerAiGenerate();

    // Wait for streaming to start
    await expect(createPage.streamingIndicator).toBeVisible({ timeout: 5000 });

    // Abort generation
    await createPage.abortGeneration();

    // Streaming indicator should disappear
    await expect(createPage.streamingIndicator).not.toBeVisible();

    // Partial content should remain in editor
    const content = await createPage.markdownEditor.textContent();
    expect(content?.length).toBeGreaterThan(0);
  });

  test('generated content is editable after completion', async ({ page }) => {
    await createPage.fillPrompt('添加暗色模式支持');
    await createPage.triggerAiGenerate();
    await createPage.waitForStreamingComplete();

    // Editor should be editable
    await createPage.markdownEditor.click();
    await page.keyboard.type('\n\n## Additional Notes\n\nCustom edit');

    const content = await createPage.markdownEditor.textContent();
    expect(content).toContain('Additional Notes');
  });

  test('re-generate overwrites previous content with confirmation', async ({ page }) => {
    await createPage.fillPrompt('第一次生成');
    await createPage.triggerAiGenerate();
    await createPage.waitForStreamingComplete();

    const firstContent = await createPage.markdownEditor.textContent();

    // Trigger re-generate
    await createPage.fillPrompt('第二次生成，不同需求');
    await createPage.triggerAiGenerate();

    // Should show confirmation dialog
    await expect(page.getByText(/覆盖当前内容/i)).toBeVisible();
    await page.getByRole('button', { name: /确认/i }).click();

    await createPage.waitForStreamingComplete();

    const secondContent = await createPage.markdownEditor.textContent();
    // Content should be different (or at least re-generated)
    expect(secondContent?.length).toBeGreaterThan(0);
  });

  test('AI generate button disabled during streaming', async ({ page }) => {
    await createPage.fillPrompt('测试需求');
    await createPage.triggerAiGenerate();

    // Button should be disabled during streaming
    await expect(createPage.aiGenerateButton).toBeDisabled();

    await createPage.waitForStreamingComplete();

    // Button should be re-enabled after completion
    await expect(createPage.aiGenerateButton).toBeEnabled();
  });
});
```

#### `e2e/kanban-responsive.spec.ts` - 移动端响应式测试

```typescript
import { test, expect, devices } from '@playwright/test';
import { KanbanPage } from './page-objects/KanbanPage';

test.describe('Kanban Responsive', () => {
  test('mobile viewport shows horizontal scroll for columns', async ({ browser }) => {
    const context = await browser.newContext({
      ...devices['iPhone 13'],
      storageState: 'e2e/.auth/admin.json',
    });
    const page = await context.newPage();
    const kanban = new KanbanPage(page);

    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // All three columns should exist but may need scrolling
    await expect(kanban.pendingColumn).toBeVisible();

    // Verify horizontal scrollability
    const container = page.getByTestId('kanban-container');
    const scrollWidth = await container.evaluate((el) => el.scrollWidth);
    const clientWidth = await container.evaluate((el) => el.clientWidth);
    expect(scrollWidth).toBeGreaterThan(clientWidth);

    await context.close();
  });

  test('tablet viewport shows all columns without scroll', async ({ browser }) => {
    const context = await browser.newContext({
      ...devices['iPad Pro 11'],
      storageState: 'e2e/.auth/admin.json',
    });
    const page = await context.newPage();
    const kanban = new KanbanPage(page);

    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // All columns should be visible
    await expect(kanban.pendingColumn).toBeVisible();
    await expect(kanban.inProgressColumn).toBeVisible();
    await expect(kanban.prColumn).toBeVisible();

    await context.close();
  });

  test('create issue page works on mobile', async ({ browser }) => {
    const context = await browser.newContext({
      ...devices['iPhone 13'],
      storageState: 'e2e/.auth/admin.json',
    });
    const page = await context.newPage();

    await page.goto('/projects/1/issues/new');
    await page.waitForLoadState('networkidle');

    // Form should be usable
    await expect(page.getByLabel(/标题/i)).toBeVisible();
    await expect(page.getByRole('button', { name: /创建 Issue/i })).toBeVisible();

    await context.close();
  });
});
```

#### `e2e/kanban-accessibility.spec.ts` - 无障碍测试

```typescript
import { test, expect } from '@playwright/test';
import { KanbanPage } from './page-objects/KanbanPage';

test.describe('Kanban Accessibility', () => {
  test('kanban columns have proper ARIA labels', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Columns should have aria-label
    await expect(kanban.pendingColumn).toHaveAttribute('aria-label', /待处理/);
    await expect(kanban.inProgressColumn).toHaveAttribute('aria-label', /处理中/);
    await expect(kanban.prColumn).toHaveAttribute('aria-label', /Pull Request/i);
  });

  test('issue cards are keyboard navigable', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Tab to first card
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab'); // Skip header elements

    // First card should be focused
    const focusedElement = page.locator(':focus');
    await expect(focusedElement).toHaveAttribute('data-testid', /card/);
  });

  test('author filter is accessible via keyboard', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Focus the filter
    await kanban.authorFilter.focus();
    await page.keyboard.press('Enter');

    // Options should be visible
    await expect(page.getByRole('listbox')).toBeVisible();

    // Navigate with arrow keys
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');

    // Filter should be applied
    await expect(kanban.authorFilter).not.toHaveText(/全部/);
  });

  test('streaming content has live region for screen readers', async ({ page }) => {
    await page.goto('/projects/1/issues/new');
    await page.waitForLoadState('networkidle');

    // The streaming content area should have aria-live
    const streamingArea = page.getByTestId('streaming-content');
    await expect(streamingArea).toHaveAttribute('aria-live', 'polite');
  });

  test('form inputs have associated labels', async ({ page }) => {
    await page.goto('/projects/1/issues/new');
    await page.waitForLoadState('networkidle');

    // Title input should have label
    const titleInput = page.getByLabel(/标题/i);
    await expect(titleInput).toBeVisible();

    // Prompt input should have label or placeholder
    const promptInput = page.getByPlaceholder(/用一句话描述/i);
    await expect(promptInput).toHaveAttribute('aria-label');
  });

  test('color contrast meets WCAG AA standards', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    // Check that status badges have sufficient contrast
    const statusBadges = page.locator('[data-testid="pr-status"]');
    const count = await statusBadges.count();

    for (let i = 0; i < Math.min(count, 3); i++) {
      const badge = statusBadges.nth(i);
      const color = await badge.evaluate((el) => window.getComputedStyle(el).color);
      const bgColor = await badge.evaluate((el) => window.getComputedStyle(el).backgroundColor);
      // Basic check - colors should be defined (full contrast ratio check needs axe-core)
      expect(color).not.toBe('');
      expect(bgColor).not.toBe('');
    }
  });
});
```

### 4.3 Visual Regression Tests

```typescript
// e2e/kanban-visual.spec.ts
import { test, expect } from '@playwright/test';
import { KanbanPage } from './page-objects/KanbanPage';

test.describe('Kanban Visual Regression', () => {
  test('kanban board full page screenshot', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    await expect(page).toHaveScreenshot('kanban-board-full.png', {
      maxDiffPixelRatio: 0.01,
    });
  });

  test('issue card visual appearance', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const firstCard = kanban.issueCards.first();
    await expect(firstCard).toHaveScreenshot('issue-card.png', {
      maxDiffPixelRatio: 0.01,
    });
  });

  test('PR card with merged status', async ({ page }) => {
    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    const mergedPr = page.locator('[data-testid="pr-card"]').filter({ hasText: /已合并/ }).first();
    if (await mergedPr.isVisible()) {
      await expect(mergedPr).toHaveScreenshot('pr-card-merged.png', {
        maxDiffPixelRatio: 0.01,
      });
    }
  });

  test('create issue page layout', async ({ page }) => {
    await page.goto('/projects/1/issues/new');
    await page.waitForLoadState('networkidle');

    await expect(page).toHaveScreenshot('create-issue-page.png', {
      maxDiffPixelRatio: 0.01,
    });
  });

  test('empty kanban state', async ({ page }) => {
    // Mock empty response
    await page.route('**/api/projects/*/kanban', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          retCode: 'SUCCESS',
          retMsg: 'ok',
          data: { pending: [], in_progress: [], pull_requests: [] },
        }),
      }),
    );

    const kanban = new KanbanPage(page);
    await kanban.goto(1);
    await kanban.waitForDataLoad();

    await expect(page).toHaveScreenshot('kanban-empty-state.png', {
      maxDiffPixelRatio: 0.01,
    });
  });
});
```


---

## 5. Phase 3 详细测试用例清单

### 5.1 看板页面组件测试

#### KanbanBoard (`components/kanban/__tests__/KanbanBoard.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders three columns with correct headers | 三列布局，标题含计数 |
| 2 | passes issues to correct columns | 数据分发正确 |
| 3 | shows loading skeleton when loading=true | 加载骨架屏 |
| 4 | shows error message on error state | 错误提示 |
| 5 | shows empty state per column | 各列空状态 |
| 6 | calls onRefresh when refresh button clicked | 刷新回调 |
| 7 | renders author filter with unique authors | 作者筛选器 |
| 8 | applies author filter to all columns | 筛选影响全部列 |

#### IssueCard (`components/kanban/__tests__/IssueCard.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders issue title | 标题渲染 |
| 2 | renders issue number (#N) | Issue 编号 |
| 3 | renders author avatar and username | 作者信息 |
| 4 | renders labels as colored chips | 标签 Chip |
| 5 | renders relative created time | 相对时间 |
| 6 | truncates long titles with ellipsis | 长标题截断 |
| 7 | shows hover state with elevation | 悬停效果 |
| 8 | opens issue detail on click (external link) | 点击跳转 |
| 9 | handles missing author gracefully | 缺失作者容错 |
| 10 | handles empty labels array | 无标签状态 |

#### PrCard (`components/kanban/__tests__/PrCard.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders PR title and number | 标题和编号 |
| 2 | renders status badge (opened/merged/closed) | 状态徽章 |
| 3 | renders correct color for merged (green) | 已合并绿色 |
| 4 | renders correct color for opened (blue) | 进行中蓝色 |
| 5 | renders correct color for closed (red) | 已关闭红色 |
| 6 | renders source and target branch | 分支信息 |
| 7 | renders linked issue count | 关联 Issue 数 |
| 8 | renders author info | 作者信息 |
| 9 | renders CI status indicator | CI 状态 |
| 10 | opens PR detail on click (external link) | 点击跳转 |
| 11 | shows review status (approved/changes_requested) | Review 状态 |

#### AuthorFilter (`components/kanban/__tests__/AuthorFilter.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders dropdown with "全部" default option | 默认选项 |
| 2 | lists all unique authors from issues | 作者列表 |
| 3 | shows author avatar in options | 头像显示 |
| 4 | calls onFilterChange with selected author | 选择回调 |
| 5 | calls onFilterChange with null on "全部" | 清除回调 |
| 6 | highlights currently selected author | 选中高亮 |
| 7 | handles empty author list | 空列表状态 |

### 5.2 Issue 创建页面组件测试

#### IssueForm (`components/issues/__tests__/IssueForm.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders all form fields | 表单完整渲染 |
| 2 | validates title required | 标题必填校验 |
| 3 | validates title max length (200 chars) | 标题长度校验 |
| 4 | calls onSubmit with form data | 提交回调数据 |
| 5 | shows loading state on submit button | 提交加载状态 |
| 6 | disables submit during submission | 提交中禁用 |
| 7 | resets form after successful submit | 成功后重置 |
| 8 | preserves form data on validation error | 校验失败保留数据 |

#### AiGenerateButton (`components/issues/__tests__/AiGenerateButton.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders button with AI icon | 按钮渲染 |
| 2 | disabled when prompt is empty | 空 prompt 禁用 |
| 3 | enabled when prompt has content | 有内容启用 |
| 4 | shows loading spinner during generation | 生成中 spinner |
| 5 | calls onGenerate with prompt text | 生成回调 |
| 6 | disabled during active streaming | 流式中禁用 |
| 7 | shows tooltip explaining feature | 功能提示 |

#### StreamingDisplay (`components/issues/__tests__/StreamingDisplay.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders empty when status is idle | 空闲状态 |
| 2 | shows streaming indicator when active | 流式指示器 |
| 3 | renders accumulated content progressively | 渐进渲染 |
| 4 | shows cursor animation during streaming | 光标动画 |
| 5 | removes cursor after completion | 完成后移除光标 |
| 6 | renders markdown content correctly | Markdown 渲染 |
| 7 | shows abort button during streaming | 中止按钮 |
| 8 | calls onAbort when abort clicked | 中止回调 |
| 9 | shows error message on stream error | 错误提示 |
| 10 | has aria-live="polite" for accessibility | 无障碍属性 |

#### MarkdownEditor (`components/issues/__tests__/MarkdownEditor.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders textarea with placeholder | 文本域渲染 |
| 2 | accepts user input | 用户输入 |
| 3 | calls onChange with content | 变更回调 |
| 4 | supports controlled value prop | 受控模式 |
| 5 | shows character count | 字符计数 |
| 6 | supports preview mode toggle | 预览切换 |
| 7 | renders markdown in preview mode | 预览渲染 |
| 8 | auto-resizes based on content | 自动高度 |

#### LabelSelector (`components/issues/__tests__/LabelSelector.test.tsx`)

| # | 测试用例 | 验证点 |
|---|----------|--------|
| 1 | renders label dropdown | 下拉渲染 |
| 2 | fetches and displays available labels | 标签列表 |
| 3 | shows label color indicator | 颜色指示 |
| 4 | allows multiple selection | 多选支持 |
| 5 | shows selected labels as chips | 已选 Chip |
| 6 | removes label on chip delete | 删除标签 |
| 7 | calls onChange with selected labels array | 变更回调 |
| 8 | shows loading state while fetching labels | 加载状态 |
| 9 | handles empty labels list | 空列表 |


---

## 6. CI/CD 集成

### 6.1 GitLab CI Pipeline 配置

```yaml
# .gitlab-ci.yml (Phase 3 前端测试部分)

stages:
  - install
  - lint
  - test-unit
  - test-e2e
  - report

variables:
  NODE_VERSION: "20"
  PLAYWRIGHT_VERSION: "1.49.1"

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
    policy: pull-push
  script:
    - cd web-frontend
    - npm ci --prefer-offline
  artifacts:
    paths:
      - web-frontend/node_modules/
    expire_in: 1 hour
  rules:
    - changes:
        - web-frontend/**/*
        - .gitlab-ci.yml

# ============================================================
# Stage: Lint + Typecheck
# ============================================================
lint-frontend:
  stage: lint
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npm run lint
  rules:
    - changes:
        - web-frontend/**/*

typecheck-frontend:
  stage: lint
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npx tsc --noEmit
  rules:
    - changes:
        - web-frontend/**/*

# ============================================================
# Stage: Unit & Integration Tests (Vitest)
# ============================================================
test-unit-frontend:
  stage: test-unit
  image: node:${NODE_VERSION}-alpine
  needs: [install-frontend]
  script:
    - cd web-frontend
    - npx vitest run --coverage --reporter=junit --outputFile=test-results/unit-results.xml
  artifacts:
    when: always
    paths:
      - web-frontend/coverage/
      - web-frontend/test-results/
    reports:
      coverage_report:
        coverage_format: cobertura
        path: web-frontend/coverage/cobertura-coverage.xml
      junit: web-frontend/test-results/unit-results.xml
  coverage: '/All files[^|]*\|[^|]*\s+([\d\.]+)/'
  rules:
    - changes:
        - web-frontend/src/**/*
        - web-frontend/vitest.config.ts

# ============================================================
# Stage: E2E Tests (Playwright)
# ============================================================
test-e2e-frontend:
  stage: test-e2e
  image: mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-jammy
  needs: [install-frontend]
  variables:
    CI: "true"
    JWT_SECRET: "dev-secret-key-at-least-32-chars-long"
    ENCRYPTION_KEY: "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="
    ADMIN_INIT_PASSWORD: "admin123"
    # AI generation mock - use test endpoint in CI
    AZURE_OPENAI_BASEURL: "http://localhost:3000/mock-ai"
    AZURE_OPENAI_API_KEY: "test-key"
  before_script:
    - cd web-frontend
    - npm ci --prefer-offline
    - npx playwright install --with-deps chromium
    # Build and start backend
    - cd .. && cargo build -p web-platform --release
    - ./target/release/web-platform &
    - cd web-frontend
    # Start frontend dev server
    - npm run dev -- --port 5177 &
    - npx wait-on http://localhost:3000/health http://localhost:5177 --timeout 60000
  script:
    - cd web-frontend
    - npx playwright test --project=chromium --reporter=junit,html
  after_script:
    - pkill -f web-platform || true
  artifacts:
    when: always
    paths:
      - web-frontend/test-results/
      - web-frontend/playwright-report/
    reports:
      junit: web-frontend/test-results/e2e-results.xml
    expire_in: 7 days
  retry:
    max: 2
    when: script_failure
  rules:
    - changes:
        - web-frontend/**/*
        - web-platform/**/*

# ============================================================
# Stage: Visual Regression (optional, manual trigger)
# ============================================================
test-visual-frontend:
  stage: test-e2e
  image: mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-jammy
  needs: [install-frontend]
  when: manual
  variables:
    CI: "true"
  before_script:
    - cd web-frontend
    - npm ci --prefer-offline
    - npx playwright install --with-deps chromium
  script:
    - cd web-frontend
    - npx playwright test e2e/kanban-visual.spec.ts --project=chromium --update-snapshots
  artifacts:
    when: always
    paths:
      - web-frontend/e2e/**/*.png
      - web-frontend/playwright-report/
    expire_in: 30 days

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
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
```

### 6.2 测试阶段流水线

```
install → lint/typecheck → unit/integration → e2e → report
   │          │                  │              │        │
   │          │                  │              │        └─ GitLab Pages 发布
   │          │                  │              │           覆盖率 + E2E 报告
   │          │                  │              │
   │          │                  │              └─ Playwright chromium
   │          │                  │                 截图 artifacts on failure
   │          │                  │                 JUnit 报告
   │          │                  │
   │          │                  └─ Vitest + coverage
   │          │                     Cobertura 覆盖率报告
   │          │                     JUnit 测试报告
   │          │
   │          └─ ESLint + TypeScript 类型检查
   │
   └─ npm ci 安装依赖
```

### 6.3 失败时的 Artifacts

- **截图**：E2E 测试失败时自动截图，保存在 `test-results/` 目录
- **Trace**：首次重试时记录完整 trace，可在 Playwright Trace Viewer 中回放
- **视频**：首次重试时录制视频
- **覆盖率报告**：HTML 格式，可在 GitLab Pages 查看


---

## 7. 测试基础设施

### 7.1 MSW Handlers（Phase 3 API）

#### `src/test/mocks/handlers-kanban.ts`

```typescript
import { http, HttpResponse } from 'msw';
import { mockKanbanData } from '../fixtures/kanban';

const BASE_URL = '*/api';

export const kanbanHandlers = [
  // GET /api/projects/:id/kanban - 获取看板数据
  http.get(`${BASE_URL}/projects/:id/kanban`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const url = new URL(request.url);
    const author = url.searchParams.get('author');

    let data = { ...mockKanbanData };

    // Apply author filter
    if (author) {
      data = {
        pending: data.pending.filter((i) => i.author.username === author),
        in_progress: data.in_progress.filter((i) => i.author.username === author),
        pull_requests: data.pull_requests.filter(
          (pr) => pr.author.username === author || pr.related_issues.some((i) => i.author.username === author),
        ),
      };
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data,
    });
  }),
];
```

#### `src/test/mocks/handlers-issues.ts`

```typescript
import { http, HttpResponse } from 'msw';

const BASE_URL = '*/api';

const mockLabels = [
  { id: 1, name: 'bug', color: '#d73a4a' },
  { id: 2, name: 'feature', color: '#0075ca' },
  { id: 3, name: 'enhancement', color: '#a2eeef' },
  { id: 4, name: 'documentation', color: '#0075ca' },
  { id: 5, name: 'priority:high', color: '#b60205' },
];

export const issueHandlers = [
  // POST /api/projects/:id/issues - 创建 Issue
  http.post(`${BASE_URL}/projects/:id/issues`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const body = (await request.json()) as { title: string; description: string; labels: string[] };

    if (!body.title || body.title.trim() === '') {
      return HttpResponse.json(
        { success: false, retCode: 'VALIDATION_001', retMsg: '标题不能为空', data: null },
        { status: 400 },
      );
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        iid: 42,
        title: body.title,
        description: body.description,
        labels: body.labels,
        state: 'opened',
        author: { username: 'admin', avatar_url: '' },
        web_url: 'https://gitlab.com/group/project/-/issues/42',
        created_at: new Date().toISOString(),
      },
    });
  }),

  // GET /api/projects/:id/labels - 获取项目标签
  http.get(`${BASE_URL}/projects/:id/labels`, ({ request }) => {
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
      data: mockLabels,
    });
  }),
];
```

#### `src/test/mocks/handlers-ai-generate.ts`

```typescript
import { http, HttpResponse } from 'msw';

const BASE_URL = '*/api';

const mockGeneratedContent = `## 描述

登录页面在移动端（宽度 < 768px）出现样式错乱，表单元素溢出容器边界，导致用户无法正常输入。

## Acceptance Criteria

- [ ] 登录表单在 375px 宽度下完整显示，无溢出
- [ ] 输入框和按钮宽度自适应容器
- [ ] 错误提示信息在移动端正确显示

## Validation

- [ ] 响应式测试: \`npx playwright test --project=mobile e2e/login.spec.ts\`
- [ ] 视觉回归: \`npx playwright test e2e/visual/login-mobile.spec.ts\`

## Notes

- 当前使用固定宽度 400px 的表单容器，需改为 max-width + 百分比宽度
- 参考设计稿中的移动端断点：768px`;

export const aiGenerateHandlers = [
  // POST /api/projects/:id/issues/ai-generate - AI 生成 Issue（SSE）
  // Note: MSW v2 does not natively support SSE streaming in node environment.
  // For unit tests, we return a regular JSON response simulating the final result.
  // For E2E tests, the real backend handles SSE.
  http.post(`${BASE_URL}/projects/:id/issues/ai-generate`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }

    const body = (await request.json()) as { prompt: string; title?: string };

    if (!body.prompt || body.prompt.trim() === '') {
      return HttpResponse.json(
        { success: false, retCode: 'VALIDATION_001', retMsg: '请输入需求描述', data: null },
        { status: 400 },
      );
    }

    // For unit tests, return the complete generated content directly
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        content: mockGeneratedContent,
      },
    });
  }),
];
```

### 7.2 Test Fixtures

#### `src/test/fixtures/kanban.ts`

```typescript
export interface MockIssue {
  iid: number;
  title: string;
  description: string;
  state: 'opened' | 'closed';
  labels: string[];
  author: {
    username: string;
    avatar_url: string;
  };
  created_at: string;
  updated_at: string;
  web_url: string;
}

export interface MockPullRequest {
  iid: number;
  title: string;
  state: 'opened' | 'merged' | 'closed';
  source_branch: string;
  target_branch: string;
  author: {
    username: string;
    avatar_url: string;
  };
  related_issues: { iid: number; title: string; author: { username: string } }[];
  ci_status: 'success' | 'failed' | 'running' | 'pending' | null;
  review_status: 'approved' | 'changes_requested' | 'pending' | null;
  web_url: string;
  created_at: string;
  updated_at: string;
}

export interface MockKanbanData {
  pending: MockIssue[];
  in_progress: MockIssue[];
  pull_requests: MockPullRequest[];
}

export const mockPendingIssues: MockIssue[] = [
  {
    iid: 101,
    title: '修复移动端登录页面样式',
    description: '## 描述\n\n登录页面在移动端样式错乱',
    state: 'opened',
    labels: ['bug', 'priority:high'],
    author: { username: 'alice', avatar_url: 'https://example.com/alice.png' },
    created_at: '2024-06-15T10:00:00Z',
    updated_at: '2024-06-15T10:00:00Z',
    web_url: 'https://gitlab.com/group/project/-/issues/101',
  },
  {
    iid: 102,
    title: '添加用户头像上传功能',
    description: '## 描述\n\n支持用户上传自定义头像',
    state: 'opened',
    labels: ['feature'],
    author: { username: 'bob', avatar_url: 'https://example.com/bob.png' },
    created_at: '2024-06-14T08:00:00Z',
    updated_at: '2024-06-14T08:00:00Z',
    web_url: 'https://gitlab.com/group/project/-/issues/102',
  },
  {
    iid: 103,
    title: '优化数据库查询性能',
    description: '## 描述\n\n项目列表查询超过 500ms',
    state: 'opened',
    labels: ['enhancement'],
    author: { username: 'alice', avatar_url: 'https://example.com/alice.png' },
    created_at: '2024-06-13T14:00:00Z',
    updated_at: '2024-06-13T14:00:00Z',
    web_url: 'https://gitlab.com/group/project/-/issues/103',
  },
];

export const mockInProgressIssues: MockIssue[] = [
  {
    iid: 98,
    title: '实现看板拖拽排序',
    description: '## 描述\n\n支持看板卡片拖拽',
    state: 'opened',
    labels: ['symphony-claimed', 'feature'],
    author: { username: 'alice', avatar_url: 'https://example.com/alice.png' },
    created_at: '2024-06-12T09:00:00Z',
    updated_at: '2024-06-15T11:00:00Z',
    web_url: 'https://gitlab.com/group/project/-/issues/98',
  },
  {
    iid: 99,
    title: '添加 WebSocket 实时通知',
    description: '## 描述\n\n实现实时推送',
    state: 'opened',
    labels: ['symphony-claimed', 'enhancement'],
    author: { username: 'bob', avatar_url: 'https://example.com/bob.png' },
    created_at: '2024-06-11T16:00:00Z',
    updated_at: '2024-06-15T09:00:00Z',
    web_url: 'https://gitlab.com/group/project/-/issues/99',
  },
];

export const mockPullRequests: MockPullRequest[] = [
  {
    iid: 45,
    title: 'feat: implement kanban drag and drop',
    state: 'opened',
    source_branch: 'feature/kanban-dnd',
    target_branch: 'main',
    author: { username: 'codex-bot', avatar_url: '' },
    related_issues: [{ iid: 98, title: '实现看板拖拽排序', author: { username: 'alice' } }],
    ci_status: 'running',
    review_status: 'pending',
    web_url: 'https://gitlab.com/group/project/-/merge_requests/45',
    created_at: '2024-06-15T12:00:00Z',
    updated_at: '2024-06-15T12:30:00Z',
  },
  {
    iid: 44,
    title: 'fix: resolve login page mobile layout',
    state: 'merged',
    source_branch: 'fix/login-mobile',
    target_branch: 'main',
    author: { username: 'codex-bot', avatar_url: '' },
    related_issues: [{ iid: 97, title: '修复登录页面移动端布局', author: { username: 'bob' } }],
    ci_status: 'success',
    review_status: 'approved',
    web_url: 'https://gitlab.com/group/project/-/merge_requests/44',
    created_at: '2024-06-14T10:00:00Z',
    updated_at: '2024-06-14T16:00:00Z',
  },
  {
    iid: 43,
    title: 'feat: add websocket notifications',
    state: 'opened',
    source_branch: 'feature/websocket',
    target_branch: 'main',
    author: { username: 'codex-bot', avatar_url: '' },
    related_issues: [{ iid: 99, title: '添加 WebSocket 实时通知', author: { username: 'bob' } }],
    ci_status: 'failed',
    review_status: 'changes_requested',
    web_url: 'https://gitlab.com/group/project/-/merge_requests/43',
    created_at: '2024-06-13T14:00:00Z',
    updated_at: '2024-06-15T08:00:00Z',
  },
];

export const mockKanbanData: MockKanbanData = {
  pending: mockPendingIssues,
  in_progress: mockInProgressIssues,
  pull_requests: mockPullRequests,
};
```

### 7.3 SSE Mock 工具

```typescript
// src/test/sse-utils.ts
import { vi } from 'vitest';

/**
 * 创建 SSE mock，用于单元测试中模拟 EventSource 行为
 */
export function createMockEventSource() {
  const listeners: Record<string, Function[]> = {};

  const mockES = {
    addEventListener: vi.fn((event: string, handler: Function) => {
      if (!listeners[event]) listeners[event] = [];
      listeners[event].push(handler);
    }),
    removeEventListener: vi.fn((event: string, handler: Function) => {
      if (listeners[event]) {
        listeners[event] = listeners[event].filter((h) => h !== handler);
      }
    }),
    close: vi.fn(),
    readyState: 0, // CONNECTING
    CONNECTING: 0,
    OPEN: 1,
    CLOSED: 2,
  };

  return {
    instance: mockES,
    // Helper to simulate server sending messages
    emit(event: string, data: any) {
      if (listeners[event]) {
        listeners[event].forEach((handler) => handler({ data: JSON.stringify(data) }));
      }
    },
    // Helper to simulate connection open
    open() {
      mockES.readyState = 1;
      if (listeners['open']) {
        listeners['open'].forEach((handler) => handler(new Event('open')));
      }
    },
    // Helper to simulate error
    error() {
      if (listeners['error']) {
        listeners['error'].forEach((handler) => handler(new Event('error')));
      }
    },
    // Helper to simulate streaming chunks
    async streamContent(content: string, chunkSize = 20, delayMs = 10) {
      this.open();
      for (let i = 0; i < content.length; i += chunkSize) {
        const chunk = content.slice(i, i + chunkSize);
        this.emit('message', { type: 'chunk', content: chunk });
        await new Promise((r) => setTimeout(r, delayMs));
      }
      this.emit('message', { type: 'done', content });
    },
  };
}

/**
 * 安装全局 EventSource mock
 */
export function installEventSourceMock() {
  const mocks: ReturnType<typeof createMockEventSource>[] = [];

  const MockEventSource = vi.fn().mockImplementation(() => {
    const mock = createMockEventSource();
    mocks.push(mock);
    return mock.instance;
  });

  vi.stubGlobal('EventSource', MockEventSource);

  return {
    getMock: (index = 0) => mocks[index],
    getLatestMock: () => mocks[mocks.length - 1],
    cleanup: () => {
      vi.unstubAllGlobals();
      mocks.length = 0;
    },
  };
}
```

### 7.4 Custom Render 扩展

```typescript
// src/test/utils.tsx 扩展（在已有基础上新增）

import { render, RenderOptions } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../theme';
import { vi } from 'vitest';
import type { ReactElement } from 'react';

// ... 保留已有的 renderWithProviders, renderAsAdmin, renderAsUser ...

/**
 * 渲染带有项目上下文的组件（用于看板和 Issue 页面测试）
 */
export function renderWithProject(
  ui: ReactElement,
  options: {
    projectId?: number;
    initialEntries?: string[];
  } = {},
) {
  const { projectId = 1, initialEntries } = options;
  const entries = initialEntries || [`/projects/${projectId}/kanban`];

  return renderAsAdmin(ui, { initialEntries: entries });
}

/**
 * 等待 SSE 流式内容完成的辅助函数
 */
export async function waitForStreamingComplete(timeout = 5000) {
  const { waitFor } = await import('@testing-library/react');
  await waitFor(
    () => {
      const indicator = document.querySelector('[data-testid="streaming-indicator"]');
      if (indicator) throw new Error('Still streaming');
    },
    { timeout },
  );
}
```

### 7.5 Playwright Page Object 基类扩展

```typescript
// e2e/page-objects/BasePage.ts (扩展)
import { Page, Locator, expect } from '@playwright/test';

export class BasePage {
  readonly page: Page;
  readonly sidebar: Locator;
  readonly breadcrumb: Locator;
  readonly toastMessage: Locator;

  constructor(page: Page) {
    this.page = page;
    this.sidebar = page.locator('[data-testid="sidebar"]');
    this.breadcrumb = page.locator('[data-testid="breadcrumb"]');
    this.toastMessage = page.locator('[role="alert"]');
  }

  async navigateTo(path: string) {
    await this.page.goto(path);
  }

  async waitForPageLoad() {
    await this.page.waitForLoadState('networkidle');
  }

  async getToastMessage(): Promise<string> {
    await this.toastMessage.waitFor({ state: 'visible', timeout: 5000 });
    return (await this.toastMessage.textContent()) ?? '';
  }

  async expectToastContains(text: string) {
    await expect(this.toastMessage).toContainText(text, { timeout: 5000 });
  }

  async navigateToKanban(projectId: number) {
    await this.sidebar.getByRole('button', { name: /看板/i }).click();
    await this.page.waitForURL(`**/projects/${projectId}/kanban`);
  }

  async navigateToCreateIssue(projectId: number) {
    await this.page.goto(`/projects/${projectId}/issues/new`);
    await this.waitForPageLoad();
  }
}
```


---

## 8. 测试执行命令

### 8.1 本地开发

```bash
# 运行所有单元/集成测试
cd web-frontend && npm run test:run

# Watch 模式（开发时使用）
cd web-frontend && npm run test

# 运行带覆盖率的测试
cd web-frontend && npm run test:coverage

# 运行 Phase 3 相关测试
cd web-frontend && npx vitest run src/components/kanban/
cd web-frontend && npx vitest run src/components/issues/
cd web-frontend && npx vitest run src/pages/__tests__/KanbanPage.test.tsx
cd web-frontend && npx vitest run src/pages/__tests__/CreateIssuePage.test.tsx
cd web-frontend && npx vitest run src/store/__tests__/kanbanStore.test.ts
cd web-frontend && npx vitest run src/store/__tests__/issueStore.test.ts
cd web-frontend && npx vitest run src/hooks/__tests__/useSSE.test.ts

# 运行 E2E 测试（需要先启动后端和前端）
cd web-frontend && npm run test:e2e

# 运行 Phase 3 E2E 测试
cd web-frontend && npx playwright test e2e/kanban.spec.ts
cd web-frontend && npx playwright test e2e/issue-creation.spec.ts
cd web-frontend && npx playwright test e2e/ai-generation.spec.ts

# E2E UI 模式（可视化调试）
cd web-frontend && npx playwright test --ui

# E2E headed 模式
cd web-frontend && npx playwright test e2e/kanban.spec.ts --headed

# 更新视觉回归快照
cd web-frontend && npx playwright test e2e/kanban-visual.spec.ts --update-snapshots

# 查看 E2E 报告
cd web-frontend && npx playwright show-report
```

### 8.2 package.json scripts 扩展

```json
{
  "scripts": {
    "test": "vitest",
    "test:run": "vitest run",
    "test:coverage": "vitest run --coverage",
    "test:e2e": "npx playwright test",
    "test:e2e:ui": "npx playwright test --ui",
    "test:e2e:headed": "npx playwright test --headed",
    "test:e2e:kanban": "npx playwright test e2e/kanban.spec.ts e2e/issue-creation.spec.ts e2e/ai-generation.spec.ts",
    "test:e2e:visual": "npx playwright test e2e/kanban-visual.spec.ts",
    "test:e2e:a11y": "npx playwright test e2e/kanban-accessibility.spec.ts",
    "test:e2e:report": "npx playwright show-report",
    "test:ci": "vitest run --reporter=junit --outputFile=test-results/unit-results.xml"
  }
}
```

---

## 9. 新增依赖

Phase 3 测试框架无需额外安装新依赖，复用已有的：

| 依赖 | 版本 | 用途 |
|------|------|------|
| `vitest` | ^2.0.0 | 单元/集成测试运行器 |
| `@testing-library/react` | ^16.0.0 | React 组件测试 |
| `@testing-library/user-event` | ^14.5.0 | 用户交互模拟 |
| `@testing-library/jest-dom` | ^6.4.0 | DOM 断言扩展 |
| `@vitest/coverage-v8` | ^2.0.0 | 覆盖率收集 |
| `msw` | ^2.3.0 | API Mock |
| `@playwright/test` | ^1.49.1 | E2E 测试 |
| `jsdom` | ^24.0.0 | DOM 环境 |

可选新增（如需 axe-core 无障碍自动化检测）：

```bash
cd web-frontend
npm install -D @axe-core/playwright
```

---

## 10. Vitest 配置扩展

```typescript
// vitest.config.ts（Phase 3 覆盖率阈值更新）
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
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
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
});
```

---

## 11. Playwright 配置扩展

```typescript
// playwright.config.ts（Phase 3 扩展）
import { defineConfig, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: process.env.CI
    ? [['html', { open: 'never' }], ['junit', { outputFile: 'test-results/e2e-results.xml' }]]
    : 'html',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:5177',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: process.env.CI ? 'on-first-retry' : 'off',
  },
  projects: [
    {
      name: 'setup',
      testMatch: /auth\.setup\.ts/,
    },
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        storageState: path.resolve(__dirname, 'e2e/.auth/admin.json'),
      },
      dependencies: ['setup'],
    },
    // Phase 3: Mobile viewport for responsive tests
    {
      name: 'mobile',
      use: {
        ...devices['iPhone 13'],
        storageState: path.resolve(__dirname, 'e2e/.auth/admin.json'),
      },
      dependencies: ['setup'],
      testMatch: /responsive|mobile/,
    },
  ],
  webServer: [
    {
      command: 'cd .. && JWT_SECRET=dev-secret-key-at-least-32-chars-long ENCRYPTION_KEY=MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY= ADMIN_INIT_PASSWORD=admin123 cargo run -p web-platform',
      url: 'http://localhost:3000/health',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
    {
      command: 'npm run dev -- --port 5177',
      url: 'http://localhost:5177',
      reuseExistingServer: !process.env.CI,
      timeout: 30000,
    },
  ],
  globalSetup: path.resolve(__dirname, 'e2e/global-setup.ts'),
  // Visual comparison settings
  expect: {
    toHaveScreenshot: {
      maxDiffPixelRatio: 0.01,
      animations: 'disabled',
    },
  },
});
```

---

## 12. 测试数据管理策略

### 单元/集成测试
- 使用 `src/test/fixtures/` 中的静态 mock 数据
- MSW handlers 提供 API 层 mock
- 每个测试用例独立，通过 `server.use()` 覆盖默认 handler 实现特殊场景
- SSE 流式数据通过 `sse-utils.ts` 中的 mock 工具模拟

### E2E 测试
- 使用真实后端 + 测试 GitLab 仓库（`http://gitlab.jushuitan-inc.com:8081/zimei10525/symphony_e2e_test_repo`）
- AI 生成接口在 CI 中使用 mock endpoint（避免依赖外部 AI 服务）
- 每次测试前通过 `global-setup.ts` 重置数据库
- 认证状态通过 `auth.setup.ts` 预先获取并存储

### AI 生成测试的特殊处理

由于 AI 生成依赖外部 Azure OpenAI 服务，测试策略分层：

| 层级 | 策略 | 说明 |
|------|------|------|
| 单元测试 | MSW mock 返回完整内容 | 不测试流式，测试组件对数据的处理 |
| 集成测试 | EventSource mock | 模拟流式 chunk 到达，测试渐进渲染 |
| E2E (本地) | 真实后端 + 真实 AI | 完整流程验证（需配置环境变量） |
| E2E (CI) | 真实后端 + mock AI endpoint | 后端提供 mock SSE 响应 |

CI 中的 mock AI endpoint 实现：

```rust
// 后端在 CI 模式下提供 mock AI 生成接口
// 当 AZURE_OPENAI_BASEURL 指向 localhost 时，返回预设的流式内容
async fn mock_ai_generate() -> impl IntoResponse {
    let content = "## 描述\n\n这是 AI 生成的测试内容。\n\n## Acceptance Criteria\n\n- [ ] 测试条件\n\n## Validation\n\n- [ ] `cargo test`";
    
    // 模拟 SSE 流式返回
    let stream = stream::iter(content.chars().collect::<Vec<_>>().chunks(20).map(|chunk| {
        let text: String = chunk.iter().collect();
        Ok::<_, Infallible>(format!("data: {{\"type\":\"chunk\",\"content\":\"{}\"}}\n\n", text))
    }));
    
    Sse::new(stream)
}
```

---

## 13. 设计系统验证要点（Phase 3）

测试中需要验证的 Architectural Logic 设计系统元素：

| 元素 | 验证方式 | 对应设计规范 |
|------|----------|-------------|
| 看板列间距 | Layout 测试 | 16px gap (4px * 4) |
| 卡片圆角 | 视觉回归 | 8px border-radius |
| 卡片无阴影 | CSS 检查 | box-shadow: none (静态卡片) |
| 状态徽章颜色 | `toHaveStyle` | merged=#2e7d32, opened=#1976d2, closed=#ba1a1a |
| 按钮渐变 | 视觉回归 | primary gradient |
| 输入框样式 | CSS 检查 | filled variant, 4px radius |
| 字体 | Computed style | Inter font-family |
| 侧边栏宽度 | Layout 测试 | 256px |
| 12-column grid | 响应式测试 | 看板列等宽分布 |
| Surface hierarchy | 视觉回归 | tonal layering 层级 |

