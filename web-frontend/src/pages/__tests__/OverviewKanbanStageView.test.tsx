import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../theme';
import { server } from '../../test/mocks/server';
import { stageKanban } from '../../test/workbenchFixtures';
import OverviewKanbanPage from '../OverviewKanbanPage';

function overviewStages() {
  const b = stageKanban();
  b.columns[0].issues = b.columns[0].issues.map((i) => ({ ...i, projectId: 5, projectName: 'portal' }));
  return {
    columns: b.columns,
    projects: [
      { projectId: 5, projectName: 'portal', platform: 'gitlab', error: null },
      { projectId: 6, projectName: 'billing', platform: 'github', error: 'no_token' },
    ],
    totalRunningProjects: 2,
    hasMore: false,
  };
}

function renderPage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter>
        <OverviewKanbanPage />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('OverviewKanbanPage stage view', () => {
  let calls: string[];
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    calls = [];
    server.use(
      http.get('*/api/overview/kanban/stages', () => {
        calls.push('stages');
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: overviewStages() });
      }),
      http.get('*/api/overview/kanban/issues', () => {
        calls.push('issues');
        return HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: { projects: [], total_running_projects: 0, has_more: false },
        });
      }),
      http.get('*/api/overview/kanban/prs', () => {
        calls.push('prs');
        return HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: { projects: [], total_running_projects: 0, has_more: false },
        });
      }),
    );
  });

  it('defaults to the merged stage board with project names and per-project errors', async () => {
    renderPage();
    const card = await screen.findByTestId('stage-card-1');
    expect(within(card).getByText('portal')).toBeInTheDocument();
    expect(screen.getByTestId('stage-col-build')).toBeInTheDocument();
    expect(screen.getByText('billing：no_token')).toBeInTheDocument();
    expect(calls).toEqual(['stages']);
  });

  it('switches to the old per-project view and remembers the choice', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByTestId('stage-card-1');
    await user.click(screen.getByRole('button', { name: '原视图' }));
    await waitFor(() => expect(calls).toContain('issues'));
    expect(screen.queryByTestId('stage-col-plan')).not.toBeInTheDocument();
    expect(localStorage.getItem('overviewKanbanView')).toBe('classic');
  });

  it('shows the empty state when no project is running', async () => {
    server.use(
      http.get('*/api/overview/kanban/stages', () =>
        HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: { columns: [], projects: [], totalRunningProjects: 0, hasMore: false },
        }),
      ),
    );
    renderPage();
    await waitFor(() => expect(screen.queryByTestId('stage-col-plan')).not.toBeInTheDocument());
    expect(await screen.findByText('暂无运行中的项目')).toBeInTheDocument();
  });

  it('shows a retryable error and refreshes on demand', async () => {
    const user = userEvent.setup();
    let fail = true;
    server.use(
      http.get('*/api/overview/kanban/stages', () => {
        calls.push('stages');
        if (fail) return HttpResponse.json({ success: false, retCode: 'SYS_001', retMsg: '服务暂不可用' }, { status: 500 });
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: overviewStages() });
      }),
    );
    renderPage();
    expect(await screen.findByText('服务暂不可用')).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByTestId('stage-card-1')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '刷新' }));
    await waitFor(() => expect(calls.filter((c) => c === 'stages').length).toBeGreaterThanOrEqual(3));
  });

  it('classic view lists running projects from the old endpoints', async () => {
    localStorage.setItem('overviewKanbanView', 'classic');
    const meta = { project_id: 5, project_name: 'portal', platform: 'gitlab', namespace: 'team', repo_name: 'portal' };
    server.use(
      http.get('*/api/overview/kanban/issues', () => {
        calls.push('issues');
        return HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: {
            projects: [{ ...meta, todo: { issues: [], total_count: 0, has_more: false }, in_progress: { issues: [], total_count: 0 }, error: null }],
            total_running_projects: 9,
            has_more: true,
          },
        });
      }),
      http.get('*/api/overview/kanban/prs', () => {
        calls.push('prs');
        return HttpResponse.json({ success: false, retCode: 'SYS_001', retMsg: 'x' }, { status: 500 });
      }),
    );
    renderPage();
    expect(await screen.findByText('portal')).toBeInTheDocument();
    expect(await screen.findByText('PR/MR 数据加载失败')).toBeInTheDocument();
    expect(screen.getByText('还有 8 个运行中项目')).toBeInTheDocument();
    expect(calls).not.toContain('stages');
  });
});
