import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import { PID, makeFlow, stageKanban } from '../../../test/workbenchFixtures';
import StageBoard from '../StageBoard';
import type { StageKanban } from '../../../types/changes';

function Where() {
  const loc = useLocation();
  return <div data-testid="location">{loc.pathname}</div>;
}

function renderBoard(data: StageKanban, onChanged = vi.fn()) {
  render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={[`/projects/${PID}/kanban`]}>
        <Routes>
          <Route
            path="*"
            element={
              <>
                <StageBoard data={data} projectId={PID} onChanged={onChanged} />
                <Where />
              </>
            }
          />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
  return { onChanged };
}

describe('StageBoard', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('renders the five stage columns with titles, counts and hints', () => {
    renderBoard(stageKanban());
    const keys = ['plan', 'design', 'build', 'deploy', 'maintain'];
    keys.forEach((k) => expect(screen.getByTestId(`stage-col-${k}`)).toBeInTheDocument());
    const plan = screen.getByTestId('stage-col-plan');
    expect(within(plan).getByText('计划')).toBeInTheDocument();
    expect(within(plan).getByText('2')).toBeInTheDocument();
    expect(within(plan).getByText('intent')).toBeInTheDocument();
    expect(within(screen.getByTestId('stage-col-build')).getByText('构建与测试')).toBeInTheDocument();
    expect(within(screen.getByTestId('stage-col-design')).getByText('暂无')).toBeInTheDocument();
    expect(within(screen.getByTestId('stage-col-maintain')).getByText('暂无')).toBeInTheDocument();
    const deploy = screen.getByTestId('stage-col-deploy');
    expect(within(deploy).getByText('!9 feat')).toBeInTheDocument();
    expect(within(deploy).getByText('symphony/app-3 · CI success')).toBeInTheDocument();
  });

  it('shows sub-state chips, waiting-for-approval, track and doc progress', () => {
    renderBoard(stageKanban());
    const c1 = screen.getByTestId('stage-card-1');
    expect(within(c1).getByText('待启动')).toBeInTheDocument();
    expect(within(c1).queryByText('等待审批')).not.toBeInTheDocument();

    const c2 = screen.getByTestId('stage-card-2');
    expect(within(c2).getByText('待审 intent')).toBeInTheDocument();
    expect(within(c2).getByText('等待审批')).toBeInTheDocument();
    expect(within(c2).getByText('track:full')).toBeInTheDocument();
    // intent in_review ◐, spec/plan not created ○
    expect(within(c2).getByLabelText('文档进度').textContent).toBe('I◐S○P○');
  });

  it('marks AI drafting running and shows per-track doc progress', () => {
    const data = stageKanban();
    const tpl = data.columns[0].issues[1];
    data.columns[1].issues = [
      {
        ...tpl,
        iid: 5,
        placement: { column: 'design', subState: '起草 spec', anomaly: false, waitingHuman: false },
        flow: {
          state: 'spec',
          track: 'full',
          docsMrUrl: null,
          mergeError: null,
          documents: [
            { docType: 'intent', status: 'approved', draftState: 'idle' },
            { docType: 'spec', status: 'draft', draftState: 'running' },
          ],
        },
      },
      {
        ...tpl,
        iid: 6,
        placement: { column: 'design', subState: '起草 plan', anomaly: false, waitingHuman: false },
        flow: {
          state: 'plan',
          track: 'plan',
          docsMrUrl: null,
          mergeError: null,
          documents: [{ docType: 'plan', status: 'draft', draftState: 'failed' }],
        },
      },
    ];
    renderBoard(data);
    const c5 = screen.getByTestId('stage-card-5');
    expect(within(c5).getByText('AI 运行中')).toBeInTheDocument();
    expect(within(c5).getByLabelText('文档进度').textContent).toBe('I●S✎P○');
    expect(within(c5).queryByText('文档 MR')).not.toBeInTheDocument();
    const c6 = screen.getByTestId('stage-card-6');
    expect(within(c6).getByLabelText('文档进度').textContent).toBe('P✎');
    expect(within(c6).queryByText('AI 运行中')).not.toBeInTheDocument();
  });

  it('shows a label anomaly warning icon only on anomalous cards', () => {
    renderBoard(stageKanban());
    expect(within(screen.getByTestId('stage-card-3')).getByLabelText('label 异常')).toBeInTheDocument();
    expect(within(screen.getByTestId('stage-card-2')).queryByLabelText('label 异常')).not.toBeInTheDocument();
    expect(within(screen.getByTestId('stage-card-1')).queryByLabelText('label 异常')).not.toBeInTheDocument();
  });

  it('links to the docs MR when the flow has one', () => {
    renderBoard(stageKanban());
    const link = within(screen.getByTestId('stage-card-2')).getByRole('link', { name: '文档 MR' });
    expect(link).toHaveAttribute('href', 'http://mr');
    expect(link).toHaveAttribute('target', '_blank');
  });

  it('shows the MR load error banner', () => {
    const data = stageKanban();
    data.prError = 'GitLab 503';
    renderBoard(data);
    expect(screen.getByText('MR 加载失败：GitLab 503')).toBeInTheDocument();
  });

  it('navigates to the workbench for cards with a flow and to the issue otherwise', async () => {
    const user = userEvent.setup();
    renderBoard(stageKanban());
    await user.click(screen.getByRole('button', { name: '#2 Issue 2' }));
    expect(screen.getByTestId('location')).toHaveTextContent(`/projects/${PID}/changes/2`);
    await user.click(screen.getByRole('button', { name: '#1 Issue 1' }));
    expect(screen.getByTestId('location')).toHaveTextContent(`/projects/${PID}/issues/1`);
  });

  it('"开始文档流程" only on 待启动 cards without flow; dialog POSTs /changes and navigates', async () => {
    const user = userEvent.setup();
    const bodies: unknown[] = [];
    server.use(
      http.post(`*/api/projects/${PID}/changes`, async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: makeFlow({ flow: { ...makeFlow().flow, issueIid: 1 } }),
        });
      }),
    );
    const { onChanged } = renderBoard(stageKanban());
    expect(within(screen.getByTestId('stage-card-2')).queryByRole('button', { name: '开始文档流程' })).not.toBeInTheDocument();
    expect(within(screen.getByTestId('stage-card-3')).queryByRole('button', { name: '开始文档流程' })).not.toBeInTheDocument();

    await user.click(within(screen.getByTestId('stage-card-1')).getByRole('button', { name: '开始文档流程' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('开始文档流程 · #1')).toBeInTheDocument();
    const select = within(dialog).getByRole('combobox');
    expect(select).toHaveTextContent('track:plan（只写 plan，默认）');
    await user.click(select);
    await user.click(await screen.findByRole('option', { name: 'track:full（intent → spec → plan）' }));
    await user.click(within(dialog).getByRole('button', { name: '开始' }));

    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent(`/projects/${PID}/changes/1`));
    expect(bodies).toEqual([{ track: 'full', issueIid: 1 }]);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('defaults to track:plan when started without changing the select', async () => {
    const user = userEvent.setup();
    const bodies: unknown[] = [];
    server.use(
      http.post(`*/api/projects/${PID}/changes`, async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: makeFlow() });
      }),
    );
    renderBoard(stageKanban());
    await user.click(within(screen.getByTestId('stage-card-1')).getByRole('button', { name: '开始文档流程' }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '开始' }));
    await waitFor(() => expect(bodies).toEqual([{ track: 'plan', issueIid: 1 }]));
  });

  it('keeps the dialog open and shows the server error when start fails', async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`*/api/projects/${PID}/changes`, () =>
        HttpResponse.json({ success: false, retCode: 'BIZ_003', retMsg: 'Issue 已有文档流程' }, { status: 409 }),
      ),
    );
    const { onChanged } = renderBoard(stageKanban());
    await user.click(within(screen.getByTestId('stage-card-1')).getByRole('button', { name: '开始文档流程' }));
    const dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: '开始' }));
    expect(await within(dialog).findByText('Issue 已有文档流程')).toBeInTheDocument();
    expect(screen.getByTestId('location')).toHaveTextContent(`/projects/${PID}/kanban`);
    expect(onChanged).not.toHaveBeenCalled();
  });
});
