import { act, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import KanbanPage, { KANBAN_AUTO_REFRESH_INTERVAL_MS } from '../projects/KanbanPage';
import { useKanbanStore } from '../../store/kanbanStore';
import type { KanbanData, KanbanIssue, PlatformUser } from '../../types/kanban';

const author: PlatformUser = {
  username: 'alice',
  display_name: 'Alice',
  avatar_url: null,
};

function makeIssue(iid: number, title: string): KanbanIssue {
  return {
    iid,
    title,
    state: 'opened',
    labels: [],
    author,
    assignees: [],
    created_at: '2026-05-01T00:00:00Z',
    updated_at: '2026-05-01T00:00:00Z',
    web_url: `https://example.com/issues/${iid}`,
    mr_count: null,
  };
}

function makeKanbanData(todoIssues: KanbanIssue[], inProgressIssues: KanbanIssue[]): KanbanData {
  return {
    todo: {
      issues: todoIssues,
      total_count: todoIssues.length,
      has_more: false,
    },
    in_progress: {
      issues: inProgressIssues,
      total_count: inProgressIssues.length,
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
    vi.clearAllMocks();
    useKanbanStore.setState({
      kanbanData: null,
      loading: false,
      error: null,
      filters: {},
      fetchKanban: vi.fn(),
      refresh: vi.fn(),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('auto refreshes kanban issue data every 15 seconds without remounting the page', async () => {
    const initialIssue = makeIssue(18, '待处理任务');
    const refreshedIssue = makeIssue(18, '处理中任务');
    const refresh = vi.fn(async () => {
      useKanbanStore.setState({
        kanbanData: makeKanbanData([], [refreshedIssue]),
      });
    });
    useKanbanStore.setState({
      kanbanData: makeKanbanData([initialIssue], []),
      refresh,
    });

    renderKanbanPage();

    expect(screen.getByText('待处理任务')).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(KANBAN_AUTO_REFRESH_INTERVAL_MS);
    });

    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledWith(1);
    expect(screen.getByText('处理中任务')).toBeInTheDocument();
    expect(screen.queryByText('待处理任务')).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '看板' })).toBeInTheDocument();
  });

  it('keeps current board visible when auto refresh reports an error', async () => {
    const refresh = vi.fn(async () => {
      useKanbanStore.setState({
        error: '刷新看板数据失败',
      });
    });
    useKanbanStore.setState({
      kanbanData: makeKanbanData([makeIssue(18, '待处理任务')], []),
      refresh,
    });

    renderKanbanPage();

    expect(screen.getByText('待处理任务')).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(KANBAN_AUTO_REFRESH_INTERVAL_MS);
    });

    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.getByText('待处理任务')).toBeInTheDocument();
    expect(screen.getByText('刷新看板数据失败')).toBeInTheDocument();
  });

  it('cleans up auto refresh timer when the page unmounts', async () => {
    const refresh = vi.fn(async () => undefined);
    useKanbanStore.setState({
      kanbanData: makeKanbanData([makeIssue(18, '待处理任务')], []),
      refresh,
    });

    const { unmount } = renderKanbanPage();

    expect(screen.getByText('待处理任务')).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();

    unmount();

    await act(async () => {
      vi.advanceTimersByTime(45_000);
    });

    expect(refresh).not.toHaveBeenCalled();
  });
});
