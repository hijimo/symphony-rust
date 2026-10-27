import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import { useKanbanStore } from '../../store/kanbanStore';
import { server } from '../../test/mocks/server';
import { stageKanban } from '../../test/workbenchFixtures';
import type { KanbanData } from '../../types/kanban';
import KanbanPage from '../projects/KanbanPage';

const original = useKanbanStore.getState();

const classicData: KanbanData = {
  todo: {
    issues: [
      {
        iid: 42,
        title: 'Classic todo issue',
        state: 'opened',
        labels: [],
        author: { username: 'octocat', display_name: 'Octocat', avatar_url: null },
        assignees: [],
        created_at: '2026-05-24T00:00:00Z',
        updated_at: '2026-05-24T00:00:00Z',
        web_url: 'https://example.test/issues/42',
        mr_count: null,
      },
    ],
    total_count: 1,
    has_more: false,
  },
  in_progress: { issues: [], total_count: 0 },
  pr: { merge_requests: [], total_count: 0 },
  cached: false,
  cached_at: null,
  platform: 'gitlab',
};

function Where() {
  return <div data-testid="location">{useLocation().pathname}</div>;
}

function renderPage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects/1/kanban']}>
        <Routes>
          <Route path="/projects/:id/kanban" element={<KanbanPage />} />
          <Route path="*" element={<Where />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('KanbanPage stage view', () => {
  let stageCalls: string[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    stageCalls = [];
    server.use(
      http.get('*/api/projects/1/kanban/stages', ({ request }) => {
        stageCalls.push(request.url);
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: stageKanban() });
      }),
    );
    useKanbanStore.setState({
      kanbanData: null,
      loading: false,
      error: null,
      filters: {},
      fetchKanban: vi.fn(async () => {
        useKanbanStore.setState({ kanbanData: classicData });
      }),
      refresh: vi.fn(),
      setFilters: vi.fn(),
      clearError: vi.fn(),
    });
  });
  afterEach(() => {
    useKanbanStore.setState(original, true);
  });

  it('defaults to the stage view when no preference is stored', async () => {
    expect(localStorage.getItem('kanbanView')).toBeNull();
    renderPage();
    expect(await screen.findByTestId('stage-col-plan')).toBeInTheDocument();
    expect(screen.getByTestId('stage-col-maintain')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '阶段视图' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: '原视图' })).toHaveAttribute('aria-pressed', 'false');
    expect(screen.queryByText('Classic todo issue')).not.toBeInTheDocument();
    expect(stageCalls.length).toBeGreaterThanOrEqual(1);
  });

  it('switches to the classic view, persists the choice and switches back', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByTestId('stage-col-plan');

    await user.click(screen.getByRole('button', { name: '原视图' }));
    expect(localStorage.getItem('kanbanView')).toBe('classic');
    expect(await screen.findByText('Classic todo issue')).toBeInTheDocument();
    expect(screen.queryByTestId('stage-col-plan')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '阶段视图' }));
    expect(localStorage.getItem('kanbanView')).toBe('stage');
    expect(await screen.findByTestId('stage-col-plan')).toBeInTheDocument();
    expect(screen.queryByText('Classic todo issue')).not.toBeInTheDocument();
  });

  it('honours a stored classic preference and does not load the stage kanban', async () => {
    localStorage.setItem('kanbanView', 'classic');
    renderPage();
    expect(await screen.findByText('Classic todo issue')).toBeInTheDocument();
    expect(screen.queryByTestId('stage-col-plan')).not.toBeInTheDocument();
    expect(stageCalls).toHaveLength(0);
  });

  it('shows a retryable error when the stage kanban fails', async () => {
    const user = userEvent.setup();
    let fail = true;
    server.use(
      http.get('*/api/projects/1/kanban/stages', () =>
        fail
          ? HttpResponse.json({ success: false, retCode: 'SYS_001', retMsg: '阶段看板加载失败' }, { status: 500 })
          : HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: stageKanban() }),
      ),
    );
    renderPage();
    expect(await screen.findByText('阶段看板加载失败')).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByTestId('stage-col-plan')).toBeInTheDocument();
  });

  it('"新建需求" navigates to the new change page', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByTestId('stage-col-plan');
    await user.click(screen.getByRole('button', { name: '新建需求' }));
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/projects/1/changes/new'));
  });
});
