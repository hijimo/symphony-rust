import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import theme from '../../theme';
import { server } from '../../test/mocks/server';
import { makeFlow } from '../../test/workbenchFixtures';
import { useIssueStore } from '../../store/issueStore';
import NewChangePage, { toIntentBody } from '../projects/NewChangePage';

const originalIssue = useIssueStore.getState();

function Where() {
  return <div data-testid="location">{useLocation().pathname}</div>;
}

function renderPage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects/7/changes/new']}>
        <Routes>
          <Route path="/projects/:id/changes/new" element={<NewChangePage />} />
          <Route path="*" element={<Where />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('toIntentBody', () => {
  it('builds all intent skeleton sections with the idea as Problem', () => {
    const body = toIntentBody('理赔进度', '  客户想看理赔进度  ', '');
    expect(body.split('\n')[0]).toBe('# Intent: 理赔进度');
    const sections = body.split('\n').filter((l) => l.startsWith('## '));
    expect(sections).toEqual(['## Problem', '## Proposed outcome', '## Affected users and systems', '## Constraints', '## Open questions']);
    expect(body).toContain('## Problem\n客户想看理赔进度\n');
    expect(body.endsWith('\n')).toBe(true);
  });

  it('prefers AI generated content over the raw idea', () => {
    const body = toIntentBody('T', 'idea', '  AI 草稿内容\n第二行  ');
    expect(body).toContain('## Problem\nAI 草稿内容\n第二行\n');
    expect(body).not.toContain('idea');
  });

  it('keeps a full AI intent document and syncs the heading with the title', () => {
    const doc = [
      '# Intent: 旧标题',
      'Author: alice. Status: draft.',
      '',
      '## Problem',
      '客户打电话问理赔进度',
      '',
      '## Proposed outcome',
      '门户里能看到进度',
      '',
      '## Affected users and systems',
      '理赔专员',
      '',
      '## Constraints',
      '不新增 PII',
      '',
      '## Open questions',
      '第三方是否需要访问？',
    ].join('\n');
    const body = toIntentBody('理赔进度自助查询', 'idea', `\n${doc}\n\n`);
    expect(body.split('\n')[0]).toBe('# Intent: 理赔进度自助查询');
    expect(body).toContain('Author: alice. Status: draft.');
    expect(body).toContain('## Constraints\n不新增 PII');
    expect(body.match(/^## Problem$/gm)).toHaveLength(1);
    expect(body).not.toContain('idea');
    expect(body.endsWith('\n')).toBe(true);
  });

  it('leaves Problem empty when both idea and draft are blank', () => {
    expect(toIntentBody('T', '  ', '')).toContain('## Problem\n\n\n## Proposed outcome');
  });
});

describe('NewChangePage', () => {
  let bodies: Array<Record<string, unknown>>;
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    bodies = [];
    server.use(
      http.post('*/api/projects/7/changes', async ({ request }) => {
        bodies.push((await request.json()) as Record<string, unknown>);
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: makeFlow({ flow: { ...makeFlow().flow, issueIid: 31 } }) });
      }),
    );
  });
  afterEach(() => useIssueStore.setState(originalIssue, true));

  it('requires a title before creating', async () => {
    const user = userEvent.setup();
    renderPage();
    const create = screen.getByRole('button', { name: '创建并进入工作台' });
    expect(create).toBeDisabled();
    await user.type(screen.getByRole('textbox', { name: '标题' }), '   ');
    expect(create).toBeDisabled();
    await user.type(screen.getByRole('textbox', { name: '标题' }), 'x');
    expect(create).toBeEnabled();
  });

  it('AI 生成草稿 needs at least 5 characters of idea', async () => {
    const user = userEvent.setup();
    renderPage();
    const gen = screen.getByRole('button', { name: 'AI 生成草稿' });
    expect(gen).toBeDisabled();
    await user.type(screen.getByRole('textbox', { name: '想法' }), '1234');
    expect(gen).toBeDisabled();
    await user.type(screen.getByRole('textbox', { name: '想法' }), '5');
    expect(gen).toBeEnabled();
  });

  it('creates a track:full flow with intent initial content and navigates to the workbench', async () => {
    const user = userEvent.setup();
    renderPage();
    await user.type(screen.getByRole('textbox', { name: '想法' }), '客户想在门户里看到理赔进度');
    await user.type(screen.getByRole('textbox', { name: '标题' }), '  理赔进度  ');
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/projects/7/changes/31'));
    expect(bodies).toHaveLength(1);
    expect(bodies[0]).toEqual({
      track: 'full',
      title: '理赔进度',
      summary: '客户想在门户里看到理赔进度',
      initialContent: toIntentBody('理赔进度', '客户想在门户里看到理赔进度', ''),
    });
    const ic = String(bodies[0].initialContent);
    expect(ic).toContain('# Intent: 理赔进度');
    expect(ic).toContain('## Problem\n客户想在门户里看到理赔进度');
    expect(ic).toContain('## Constraints');
  });

  it('uses the AI draft once generation is done and pre-fills the title', async () => {
    const user = userEvent.setup();
    renderPage();
    // Simulate the issue AI store finishing a generation.
    useIssueStore.setState({ aiStatus: 'done', generatedContent: '生成的问题描述', generatedTitle: 'AI 标题', aiError: null });
    const title = (await screen.findByRole('textbox', { name: '标题' })) as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('AI 标题'));
    expect(screen.getByText('生成的问题描述')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0].initialContent).toBe(toIntentBody('AI 标题', '', '生成的问题描述'));
    expect(bodies[0].summary).toBeUndefined();
  });

  it('asks the AI for the intent template', async () => {
    const user = userEvent.setup();
    const calls: unknown[] = [];
    useIssueStore.setState({ startGenerate: (pid: number, data: unknown) => calls.push([pid, data]) });
    renderPage();
    await user.type(screen.getByRole('textbox', { name: '想法' }), '客户想看理赔进度');
    await user.click(screen.getByRole('button', { name: 'AI 生成草稿' }));
    expect(calls).toEqual([[7, { prompt: '客户想看理赔进度', template: 'intent' }]]);
  });

  it('shows the server error and stays on the page', async () => {
    const user = userEvent.setup();
    server.use(
      http.post('*/api/projects/7/changes', () =>
        HttpResponse.json({ success: false, retCode: 'BIZ_001', retMsg: '项目创建者未配置 GitLab Token' }, { status: 400 }),
      ),
    );
    renderPage();
    await user.type(screen.getByRole('textbox', { name: '标题' }), 'T');
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    expect(await screen.findByText('项目创建者未配置 GitLab Token')).toBeInTheDocument();
    expect(screen.queryByTestId('location')).not.toBeInTheDocument();
  });
});
