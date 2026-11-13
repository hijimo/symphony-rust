import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import { server } from '../../test/mocks/server';
import { makeFlow } from '../../test/workbenchFixtures';
import { useIssueStore } from '../../store/issueStore';
import NewChangePage, { generateIntentDraft, toIntentBody } from '../projects/NewChangePage';

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

const AI_URL = '*/api/projects/7/issues/ai-generate';
function sse(events: object[]) {
  const body = events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join('');
  return new HttpResponse(body, { headers: { 'Content-Type': 'text/event-stream' } });
}
const AI_DOC = '# Intent: AI 标题\n\n## Problem\nAI 写的问题\n\n## Proposed outcome\n好\n';

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

  it('creates a track:full flow with the fallback intent when the AI fails', async () => {
    const user = userEvent.setup();
    server.use(http.post(AI_URL, () => sse([{ type: 'error', error: 'AI 服务不可用' }])));
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

  it('generates the AI draft automatically when creating without one', async () => {
    const user = userEvent.setup();
    let aiCalls = 0;
    server.use(
      http.post(AI_URL, () => {
        aiCalls += 1;
        return sse([{ type: 'done', content: AI_DOC, title: 'AI 标题' }]);
      }),
    );
    renderPage();
    await user.type(screen.getByRole('textbox', { name: '想法' }), '客户想在门户里看到理赔进度');
    await user.type(screen.getByRole('textbox', { name: '标题' }), '理赔进度');
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(aiCalls).toBe(1);
    // The AI document is kept, with the user's title.
    expect(bodies[0].initialContent).toBe(toIntentBody('理赔进度', '客户想在门户里看到理赔进度', AI_DOC));
    expect(String(bodies[0].initialContent)).toContain('## Problem\nAI 写的问题');
  });

  it('skips the automatic draft when the idea is shorter than 5 characters', async () => {
    const user = userEvent.setup();
    let aiCalls = 0;
    server.use(http.post(AI_URL, () => { aiCalls += 1; return sse([]); }));
    renderPage();
    await user.type(screen.getByRole('textbox', { name: '标题' }), 'T');
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(aiCalls).toBe(0);
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

  it('uploads chosen attachments into the new flow after creating it', async () => {
    const user = userEvent.setup();
    const uploads: string[] = [];
    server.use(
      http.post('*/api/projects/7/changes/31/upload', ({ request }) => {
        uploads.push(new URL(request.url).searchParams.get('path') ?? '');
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { path: '', version: 'v1', commitSha: 'a' } });
      }),
    );
    renderPage();
    await user.upload(screen.getByLabelText('添加附件'), new File(['PK'], '评分标准.xlsx'));
    expect(screen.getByText(/评分标准\.xlsx/)).toBeInTheDocument();
    const dir = screen.getByLabelText('附件目录');
    await user.clear(dir);
    await user.type(dir, 'assets/评分');
    await user.type(screen.getByRole('textbox', { name: '标题' }), '评分模块');
    await user.click(screen.getByRole('button', { name: '创建并进入工作台' }));
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/projects/7/changes/31'));
    expect(uploads).toEqual(['docs/changes/12/assets/评分/评分标准.xlsx']);
  });

  it('blocks creation while the attachment directory is invalid', async () => {
    const user = userEvent.setup();
    renderPage();
    await user.upload(screen.getByLabelText('添加附件'), new File(['x'], 'a.pdf'));
    const dir = screen.getByLabelText('附件目录');
    await user.clear(dir);
    await user.type(dir, '../x');
    await user.type(screen.getByRole('textbox', { name: '标题' }), 'T');
    expect(screen.getByRole('button', { name: '创建并进入工作台' })).toBeDisabled();
    await user.click(screen.getByLabelText('移除 a.pdf'));
    expect(screen.getByRole('button', { name: '创建并进入工作台' })).toBeEnabled();
  });
});

describe('generateIntentDraft', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('returns the draft and title on success', async () => {
    server.use(http.post(AI_URL, () => sse([{ type: 'done', content: AI_DOC, title: 'T' }])));
    expect(await generateIntentDraft(7, '想法想法想法')).toEqual({ content: AI_DOC, title: 'T', error: null });
  });

  it('reports errors and empty drafts instead of throwing', async () => {
    server.use(http.post(AI_URL, () => sse([{ type: 'error', error: '超时了' }])));
    expect((await generateIntentDraft(7, '想法想法想法')).error).toBe('超时了');
    server.use(http.post(AI_URL, () => sse([{ type: 'done', content: '  ' }])));
    expect((await generateIntentDraft(7, '想法想法想法')).error).toBe('AI 返回内容为空');
  });

  it('gives up after the timeout', async () => {
    server.use(http.post(AI_URL, async () => { await new Promise((r) => setTimeout(r, 500)); return sse([]); }));
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const p = generateIntentDraft(7, '想法想法想法', 100);
    await vi.advanceTimersByTimeAsync(150);
    const r = await p;
    vi.useRealTimers();
    expect(r.error).toContain('未完成');
    expect(r.content).toBe('');
  });
});
