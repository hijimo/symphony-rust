import { act, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import { useKanbanStore } from '../../store/kanbanStore';
import type { KanbanData, KanbanIssue, KanbanMergeRequest } from '../../types/kanban';
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

function createMergeRequest(
  iid: number,
  title: string,
  state: KanbanMergeRequest['state'],
): KanbanMergeRequest {
  return {
    iid,
    title,
    state,
    repository: 'org/repo',
    author,
    source_branch: `feature/${iid}`,
    target_branch: 'main',
    ci_status: 'success',
    review_status: 'pending',
    related_issue_iids: [],
    created_at: '2026-05-23T00:00:00Z',
    updated_at: '2026-05-24T00:00:00Z',
    web_url: `https://example.test/pulls/${iid}`,
  };
}

function createKanbanData(
  issue?: KanbanIssue,
  mergeRequests: KanbanMergeRequest[] = [],
  prError?: string,
): KanbanData {
  return {
    todo: {
      issues: issue ? [issue] : [],
      total_count: issue ? 1 : 0,
      has_more: false,
    },
    in_progress: {
      issues: [],
      total_count: 0,
    },
    pr: {
      merge_requests: mergeRequests,
      total_count: mergeRequests.length,
      error: prError,
    },
    cached: false,
    cached_at: null,
    platform: 'github',
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

  it('renders PR column items with required fields', async () => {
    const fetchKanban = vi.fn(async () => {});
    const pendingPr = createMergeRequest(27, 'Review kanban PR filtering', 'pending');

    useKanbanStore.setState({
      kanbanData: createKanbanData(undefined, [pendingPr]),
      loading: false,
      error: null,
      filters: {},
      fetchKanban,
      refresh: vi.fn(),
      setFilters: vi.fn(),
      clearError: vi.fn(),
    });

    renderKanbanPage();

    expect(screen.getByRole('link', { name: /Review kanban PR filtering/ })).toHaveAttribute(
      'href',
      pendingPr.web_url,
    );
    expect(screen.getByText('Octocat')).toBeInTheDocument();
    expect(screen.getByText('feature/27 → main')).toBeInTheDocument();
    expect(screen.getAllByText('待处理').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText(/小时前|分钟前|刚刚|天前|个月前/)).toBeInTheDocument();
  });

  it('renders PR empty state', async () => {
    const fetchKanban = vi.fn(async () => {});

    useKanbanStore.setState({
      kanbanData: createKanbanData(),
      loading: false,
      error: null,
      filters: {},
      fetchKanban,
      refresh: vi.fn(),
      setFilters: vi.fn(),
      clearError: vi.fn(),
    });

    renderKanbanPage();

    expect(screen.getByText('暂无待处理 PR')).toBeInTheDocument();
  });

  it('renders PR column error state', async () => {
    const fetchKanban = vi.fn(async () => {});

    useKanbanStore.setState({
      kanbanData: createKanbanData(undefined, [], 'PR/MR 数据加载失败：mock failure'),
      loading: false,
      error: null,
      filters: {},
      fetchKanban,
      refresh: vi.fn(),
      setFilters: vi.fn(),
      clearError: vi.fn(),
    });

    renderKanbanPage();

    expect(screen.getByText('PR/MR 数据加载失败：mock failure')).toBeInTheDocument();
  });
});
