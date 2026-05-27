import { act, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import { useKanbanStore } from '../../store/kanbanStore';
import { server } from '../../test/mocks/server';
import type { KanbanData, KanbanIssue } from '../../types/kanban';
import KanbanPage from '../projects/KanbanPage';

const originalKanbanState = useKanbanStore.getState();

const author = {
  username: 'octocat',
  display_name: 'Octocat',
  avatar_url: null,
};

function createIssue(iid: number, title: string): KanbanIssue {
  return {
    iid,
    title,
    state: 'opened',
    labels: [],
    author,
    assignees: [],
    created_at: '2026-05-24T00:00:00Z',
    updated_at: '2026-05-24T00:00:00Z',
    web_url: `https://example.test/issues/${iid}`,
    mr_count: null,
  };
}

function createKanbanData(issue: KanbanIssue): KanbanData {
  return {
    todo: {
      issues: [issue],
      total_count: 1,
      has_more: false,
    },
    in_progress: {
      issues: [],
      total_count: 0,
    },
    pr: {
      merge_requests: [],
      total_count: 0,
    },
    cached: false,
    cached_at: null,
    platform: 'github',
  };
}

function mockKanbanStore(issueTitle = 'Initial issue') {
  const fetchKanban = vi.fn(async () => {
    useKanbanStore.setState({
      kanbanData: createKanbanData(createIssue(1, issueTitle)),
    });
  });

  useKanbanStore.setState({
    kanbanData: null,
    loading: false,
    error: null,
    filters: {},
    fetchKanban,
    refresh: vi.fn(),
    setFilters: vi.fn(),
    clearError: vi.fn(),
  });

  return { fetchKanban };
}

function createProjectResponse(name: string) {
  return {
    id: 1,
    name,
    description: 'A test project on GitLab',
    git_url: 'https://gitlab.com/group/my-project.git',
    platform: 'gitlab',
    platform_host: 'gitlab.com',
    namespace: 'group',
    repo_name: 'my-project',
    default_branch: 'main',
    workflow_template: 'default',
    service_status: 'running',
    service_pid: 12345,
    max_concurrent_agents: 2,
    auto_restart: true,
    member_count: 3,
    my_role: 'owner',
    created_by: 1,
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-10T00:00:00Z',
    hooks_after_create: null,
    hooks_before_remove: null,
    codex_command: null,
    codex_approval_policy: null,
    codex_sandbox: null,
  };
}

function renderKanbanPage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects/1/kanban']}>
        <Routes>
          <Route path="/projects/:id/kanban" element={<KanbanPage />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('KanbanPage', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  afterEach(() => {
    vi.useRealTimers();
    useKanbanStore.setState(originalKanbanState, true);
  });

  it('refreshes kanban issue data every 15 seconds and stops after unmount', async () => {
    const fetchKanban = vi.fn(async () => {
      useKanbanStore.setState({
        kanbanData: createKanbanData(createIssue(1, 'Initial issue')),
      });
    });
    const refresh = vi.fn(async () => {
      useKanbanStore.setState({
        kanbanData: createKanbanData(createIssue(2, 'Updated issue')),
      });
    });

    useKanbanStore.setState({
      kanbanData: null,
      loading: false,
      error: null,
      filters: {},
      fetchKanban,
      refresh,
      setFilters: vi.fn(),
      clearError: vi.fn(),
    });

    const { unmount } = renderKanbanPage();

    await act(async () => {});

    expect(screen.getByText('Initial issue')).toBeInTheDocument();
    expect(fetchKanban).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(14_999);
    });

    expect(refresh).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(1);
    });

    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Updated issue')).toBeInTheDocument();

    unmount();

    await act(async () => {
      vi.advanceTimersByTime(15_000);
    });

    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('shows the current project name from project data', async () => {
    vi.useRealTimers();
    mockKanbanStore();

    renderKanbanPage();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });

    expect(screen.getByText('Initial issue')).toBeInTheDocument();
  });

  it('shows an unnamed project fallback when project name is blank', async () => {
    vi.useRealTimers();
    mockKanbanStore();
    server.use(
      http.get('*/api/projects/:id', () => {
        return HttpResponse.json({
          success: true,
          retCode: 'SUCCESS',
          retMsg: 'ok',
          data: createProjectResponse('   '),
        });
      }),
    );

    renderKanbanPage();

    await waitFor(() => {
      expect(screen.getByText('未命名项目')).toBeInTheDocument();
    });
    expect(screen.getByText('Initial issue')).toBeInTheDocument();
  });

  it('shows a project name fallback when project data fails to load', async () => {
    vi.useRealTimers();
    mockKanbanStore();
    server.use(
      http.get('*/api/projects/:id', () => {
        return HttpResponse.json(
          { success: false, retCode: 'NOT_FOUND', retMsg: '项目不存在', data: null },
          { status: 404 },
        );
      }),
    );

    renderKanbanPage();

    await waitFor(() => {
      expect(screen.getByText('项目名称不可用')).toBeInTheDocument();
    });
    expect(screen.getByText('Initial issue')).toBeInTheDocument();
  });
});
