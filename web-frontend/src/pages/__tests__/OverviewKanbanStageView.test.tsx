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
});
