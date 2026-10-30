import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../theme';
import { server } from '../../test/mocks/server';
import { IID, INTENT_TEXT, MockSocket, PID, makeFlow, workbenchHandlers, type WbCalls } from '../../test/workbenchFixtures';
import WorkbenchPage from '../projects/WorkbenchPage';
import { useWorkbenchStore } from '../../store/workbenchStore';

// jsdom has no layout (every rect is 0x0), so react-resizable-panels treats
// every pointerdown as a separator drag and steals focus. Layout is not under
// test here; replace the panels with plain containers.
vi.mock('react-resizable-panels', () => {
  const Box = ({ children }: { children?: React.ReactNode }) => <div>{children}</div>;
  return { Group: Box, Panel: Box, Separator: () => <div /> };
});

const INTENT = `docs/changes/${IID}/intent.md`;
const base = `*/api/projects/${PID}/changes/${IID}`;

function renderPage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={[`/projects/${PID}/changes/${IID}`]}>
        <Routes>
          <Route path="/projects/:id/changes/:iid" element={<WorkbenchPage />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

async function ready() {
  // Markdown opens in preview by default; switch to 源码 to reach the editor.
  await userEvent.click(await screen.findByRole('button', { name: '源码' }));
  const editor = (await screen.findByLabelText('编辑器')) as HTMLTextAreaElement;
  await waitFor(() => expect(editor.value).toBe(INTENT_TEXT));
  return editor;
}

async function socket() {
  await waitFor(() => expect(MockSocket.instances.length).toBeGreaterThan(0));
  const ws = MockSocket.instances[MockSocket.instances.length - 1];
  await waitFor(() => expect(ws.readyState).toBe(1));
  await waitFor(() => expect(useWorkbenchStore.getState().connected).toBe(true));
  return ws;
}

function frames(ws: MockSocket) {
  return ws.sent.map((s) => JSON.parse(s) as Record<string, unknown>);
}

describe('WorkbenchPage', () => {
  let calls: WbCalls;

  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    MockSocket.instances = [];
    vi.stubGlobal('WebSocket', MockSocket);
    calls = { writes: [], decisions: [], submits: [] };
  });

  afterEach(() => {
    useWorkbenchStore.getState().dispose();
    vi.unstubAllGlobals();
  });

  it('loads flow, workspace and tree, opens the stage document with preview', async () => {
    server.use(...workbenchHandlers(makeFlow(), calls));
    renderPage();
    // Markdown documents open in 预览 mode by default: no source editor.
    const preview = await screen.findByTestId('markdown-preview');
    await waitFor(() => expect(within(preview).getByRole('heading', { name: 'Intent: X' })).toBeInTheDocument());
    expect(screen.getByRole('button', { name: '预览' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByLabelText('编辑器')).not.toBeInTheDocument();
    expect(screen.getByText('需求文档工作台')).toBeInTheDocument();
    // Root tree entries
    expect(screen.getByTestId('tree-docs')).toBeInTheDocument();
    expect(screen.getByTestId('tree-src')).toBeInTheDocument();
    // Intent tab and preview (frontmatter stripped, GFM table)
    expect(screen.getByRole('tab', { name: /intent\.md/ })).toBeInTheDocument();
    expect(within(preview).getByRole('table')).toBeInTheDocument();
    expect(preview.textContent).not.toContain('symphony:');
    // Flow state chip
    expect(screen.getByTestId('flow-state')).toHaveTextContent('待审 intent（G1）');
  });

  it('expands a directory and opens a read-only file in a new tab', async () => {
    const user = userEvent.setup();
    server.use(...workbenchHandlers(makeFlow(), calls));
    renderPage();
    await ready();
    await user.click(screen.getByTestId('tree-src'));
    await user.click(await screen.findByTestId('tree-src/main.rs'));
    const editor = (await screen.findByLabelText('编辑器')) as HTMLTextAreaElement;
    await waitFor(() => expect(editor.value).toBe('fn main() {}\n'));
    expect(editor).toHaveAttribute('readonly');
    expect(screen.getByRole('tab', { name: /main\.rs/ })).toBeInTheDocument();
    // Read-only files have no 保存 button
    expect(screen.queryByRole('button', { name: '保存' })).not.toBeInTheDocument();
  });

  it('autosaves edits with commit=false, then 保存 commits with commit=true', async () => {
    const user = userEvent.setup();
    server.use(...workbenchHandlers(makeFlow(), calls));
    renderPage();
    const editor = await ready();
    await user.type(editor, 'more');
    expect(screen.getByLabelText('未保存')).toBeInTheDocument();
    await waitFor(() => expect(calls.writes).toHaveLength(1), { timeout: 4000 });
    expect(calls.writes[0]).toEqual({ path: INTENT, content: `${INTENT_TEXT}more`, baseVersion: 'v1', commit: false });
    await waitFor(() => expect(screen.queryByLabelText('未保存')).not.toBeInTheDocument());

    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(calls.writes).toHaveLength(2));
    expect(calls.writes[1]).toEqual({ path: INTENT, content: `${INTENT_TEXT}more`, baseVersion: 'v2', commit: true });
    expect(await screen.findByText('已提交 deadbeef')).toBeInTheDocument();
  });

  it('409 conflict shows the conflict bar; 保留我的 re-saves on the new version, 采用新的 takes disk content', async () => {
    const user = userEvent.setup();
    let disk = { content: INTENT_TEXT, version: 'v1' };
    const writes: Array<Record<string, unknown>> = [];
    server.use(
      http.get(`${base}/file`, ({ request }) => {
        const path = new URL(request.url).searchParams.get('path');
        return HttpResponse.json({
          success: true, retCode: '0', retMsg: 'ok',
          data: { path, content: disk.content, version: disk.version, size: 1, binary: false, tooLarge: false, writable: true },
        });
      }),
      http.put(`${base}/file`, async ({ request }) => {
        const b = (await request.json()) as Record<string, unknown>;
        writes.push(b);
        if (b.baseVersion !== disk.version) {
          return HttpResponse.json({ success: false, retCode: 'BIZ_003', retMsg: '文件已被修改，请查看差异后再保存' }, { status: 409 });
        }
        disk = { content: String(b.content), version: `${disk.version}x` };
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { version: disk.version, commitSha: null } });
      }),
      ...workbenchHandlers(makeFlow(), calls),
    );
    renderPage();
    const editor = await ready();
    // Someone else changes the file on disk.
    disk = { content: 'THEIRS\n', version: 'v7' };
    await user.type(editor, 'mine');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(await screen.findByText('文件已被 AI 或其他人修改')).toBeInTheDocument();
    expect(await screen.findByText('文件已被修改，请查看差异后再保存')).toBeInTheDocument();
    expect(editor.value).toBe(`${INTENT_TEXT}mine`);

    await user.click(screen.getByRole('button', { name: '保留我的' }));
    await waitFor(() => expect(screen.queryByText('文件已被 AI 或其他人修改')).not.toBeInTheDocument());
    await waitFor(() => expect(writes.some((w) => w.baseVersion === 'v7' && w.content === `${INTENT_TEXT}mine` && w.commit === false)).toBe(true));
    expect(disk.content).toBe(`${INTENT_TEXT}mine`);

    // Second conflict: take theirs.
    disk = { content: 'THEIRS AGAIN\n', version: 'v9' };
    await user.type(editor, '!');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(await screen.findByText('文件已被 AI 或其他人修改')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '采用新的' }));
    await waitFor(() => expect(editor.value).toBe('THEIRS AGAIN\n'));
    expect(screen.queryByText('文件已被 AI 或其他人修改')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('未保存')).not.toBeInTheDocument();
  });

  it('shows the behind-main hint and merges main on demand', async () => {
    const user = userEvent.setup();
    let merged = 0;
    server.use(
      http.post(`${base}/workspace/merge-main`, () => {
        merged += 1;
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: null });
      }),
      ...workbenchHandlers(makeFlow(), calls),
    );
    renderPage();
    await ready();
    expect(screen.getByText('主干有更新')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '更新到最新主干' }));
    await waitFor(() => expect(merged).toBe(1));
  });

  it('does not show the behind-main hint when up to date', async () => {
    server.use(
      http.post(`${base}/workspace`, () =>
        HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { context: { baseSha: 'abc', behindMain: false, missing: [] }, writableDir: `docs/changes/${IID}`, multiRepo: false, primaryDir: '' } }),
      ),
      ...workbenchHandlers(makeFlow(), calls),
    );
    renderPage();
    await ready();
    expect(screen.queryByText('主干有更新')).not.toBeInTheDocument();
  });

  it('shows the load error when the flow cannot be loaded', async () => {
    server.use(
      http.get(base, () => HttpResponse.json({ success: false, retCode: 'BIZ_002', retMsg: '文档流程不存在' }, { status: 404 })),
    );
    renderPage();
    expect(await screen.findByText('文档流程不存在')).toBeInTheDocument();
    expect(screen.queryByTestId('gate-panel')).not.toBeInTheDocument();
  });

  it('WS: sends a message, renders streamed agent output, approval answer frames and turn completion', async () => {
    const user = userEvent.setup();
    server.use(...workbenchHandlers(makeFlow(), calls));
    renderPage();
    await ready();
    const ws = await socket();
    expect(ws.url).toContain(`/api/projects/${PID}/changes/${IID}/ws?ticket=t1`);

    const input = screen.getByLabelText('AI 输入');
    await user.type(input, '把 Problem 写清楚');
    await user.click(screen.getByRole('button', { name: '发送' }));
    expect(frames(ws)).toContainEqual({ type: 'send', stage: 'intent', text: '把 Problem 写清楚', files: [] });
    expect((input as HTMLTextAreaElement).value).toBe('');

    act(() => {
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'user_message', text: '把 Problem 写清楚', mode: 'interactive' } });
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'turn_started', turnId: 't' } });
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'message_delta', itemId: 'm1', delta: '正在' } });
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'message_delta', itemId: 'm1', delta: '修改' } });
    });
    const chat = screen.getByTestId('chat');
    expect(within(chat).getByText('把 Problem 写清楚')).toBeInTheDocument();
    expect(within(chat).getByTestId('assistant-message')).toHaveTextContent('正在修改');
    expect(screen.getByLabelText('AI 运行中')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '追加说明' })).toBeInTheDocument();
    // Editor is locked while the AI runs on this stage.
    expect(screen.getByText('AI 正在修改文档，本轮结束后可以编辑')).toBeInTheDocument();

    // Approval: allow
    act(() => {
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'approval_request', requestId: 'r1', kind: 'command', command: 'ls docs', cwd: null, reason: null } });
    });
    const card = await screen.findByTestId('approval-card');
    expect(card).toHaveTextContent('ls docs');
    await user.click(within(card).getByRole('button', { name: '允许' }));
    expect(frames(ws)).toContainEqual({ type: 'answer', stage: 'intent', requestId: 'r1', answer: { decision: 'accept' } });
    expect(screen.queryByTestId('approval-card')).not.toBeInTheDocument();

    // Approval: decline
    act(() => {
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'approval_request', requestId: 'r2', kind: 'command', command: 'rm -rf /tmp/x', cwd: null, reason: null } });
    });
    const card2 = await screen.findByTestId('approval-card');
    await user.click(within(card2).getByRole('button', { name: '拒绝' }));
    expect(frames(ws)).toContainEqual({ type: 'answer', stage: 'intent', requestId: 'r2', answer: { decision: 'decline' } });

    // Steering while running
    await user.type(input, '再简短一点');
    await user.click(screen.getByRole('button', { name: '追加说明' }));
    expect(frames(ws)).toContainEqual({ type: 'steer', stage: 'intent', text: '再简短一点' });

    act(() => {
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'item', phase: 'completed', item: { id: 'm1', kind: 'message', text: '已完成修改' } } });
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'turn_completed', status: 'completed', error: null } });
    });
    expect(within(chat).getByText('— 本轮完成 —')).toBeInTheDocument();
    expect(within(chat).getByTestId('assistant-message')).toHaveTextContent('已完成修改');
    expect(screen.queryByLabelText('AI 运行中')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '发送' })).toBeInTheDocument();
    expect(screen.queryByText('AI 正在修改文档，本轮结束后可以编辑')).not.toBeInTheDocument();
  });

  it('WS: failed turn shows the error status line', async () => {
    server.use(...workbenchHandlers(makeFlow(), calls));
    renderPage();
    await ready();
    const ws = await socket();
    act(() => {
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'turn_started', turnId: 't' } });
      ws.emit({ type: 'agent', stage: 'intent', event: { type: 'turn_completed', status: 'failed', error: 'boom' } });
    });
    expect(screen.getByText('— 本轮结束：failed（boom） —')).toBeInTheDocument();
  });

  it('WS: sending before the socket is open shows a notice and sends nothing', async () => {
    const user = userEvent.setup();
    // ws-ticket never resolves → no socket.
    server.use(
      http.post(`${base}/ws-ticket`, () => new Promise<never>(() => undefined)),
      ...workbenchHandlers(makeFlow(), calls),
    );
    renderPage();
    await ready();
    await user.type(screen.getByLabelText('AI 输入'), 'hi');
    await user.click(screen.getByRole('button', { name: '发送' }));
    expect(await screen.findByText('AI 连接未就绪，请稍后重试')).toBeInTheDocument();
    expect((screen.getByLabelText('AI 输入') as HTMLTextAreaElement).value).toBe('hi');
  });

  // KNOWN DEFECT (reported, intentionally failing): dispose() does not cancel an
  // in-flight connect(); the guard in connect() only compares projectId/iid, which
  // dispose() leaves unchanged. Leaving the workbench before the ws-ticket
  // resolves therefore still opens a socket (and it auto-reconnects forever).
  // The same race is what opens two sockets under React StrictMode (E2E).
  it('does not open a WebSocket after the page is left before the ticket resolves', async () => {
    let releaseTicket!: () => void;
    const gate = new Promise<void>((r) => (releaseTicket = r));
    server.use(
      http.post(`${base}/ws-ticket`, async () => {
        await gate;
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { ticket: 't-late' } });
      }),
      ...workbenchHandlers(makeFlow(), calls),
    );
    const { unmount } = renderPage();
    await ready();
    unmount();
    releaseTicket();
    await new Promise((r) => setTimeout(r, 100));
    expect(MockSocket.instances.filter((s) => s.readyState !== 3)).toHaveLength(0);
  });
});
