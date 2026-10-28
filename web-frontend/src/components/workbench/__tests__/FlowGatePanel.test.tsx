import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import { IID, PID, makeFlow, workbenchHandlers, type WbCalls } from '../../../test/workbenchFixtures';
import { useWorkbenchStore } from '../../../store/workbenchStore';
import FlowGatePanel from '../FlowGatePanel';
import type { ChangeDocument, DocType, FlowView } from '../../../types/changes';

const base = `*/api/projects/${PID}/changes/${IID}`;
const empty = () => ({ chat: [], running: false, pending: [], lastDiff: null });

function doc(docType: DocType, over: Partial<ChangeDocument> = {}): ChangeDocument {
  return { ...makeFlow().documents[0], id: { intent: 10, spec: 11, plan: 12 }[docType], docType, path: `docs/changes/${IID}/${docType}.md`, ...over };
}

function setup(flow: FlowView, stage: DocType, calls: WbCalls) {
  server.use(...workbenchHandlers(flow, calls));
  useWorkbenchStore.setState({
    projectId: PID,
    iid: IID,
    flow,
    stage,
    stages: { intent: empty(), spec: empty(), plan: empty() },
    context: { baseSha: 'abc', behindMain: false, missing: [] },
    buffers: {},
    tabs: [],
    active: null,
    notices: [],
  });
  render(
    <ThemeProvider theme={theme}>
      <FlowGatePanel />
    </ThemeProvider>,
  );
}

describe('FlowGatePanel', () => {
  let calls: WbCalls;
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    calls = { writes: [], decisions: [], submits: [] };
  });
  afterEach(() => useWorkbenchStore.getState().dispose());

  it('draft: 校验 shows template validation issues and 提交审阅 submits the stage doc', async () => {
    const user = userEvent.setup();
    setup(makeFlow({ state: 'intent', documents: [doc('intent', { status: 'draft' })] }), 'intent', calls);
    expect(screen.getByTestId('flow-state')).toHaveTextContent('起草 intent');
    expect(screen.queryByRole('button', { name: /批准/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '校验' }));
    const issues = await screen.findByTestId('validation-issues');
    expect(issues).toHaveTextContent('Constraints：章节 ## Constraints 不能为空');
    await user.click(screen.getByRole('button', { name: '提交审阅' }));
    await waitFor(() => expect(calls.submits).toEqual(['intent']));
  });

  it('校验 with no issues shows 校验通过', async () => {
    const user = userEvent.setup();
    setup(makeFlow({ state: 'intent', documents: [doc('intent', { status: 'changes_requested', lastFeedback: '补充约束' })] }), 'intent', calls);
    server.use(http.post(`${base}/docs/:doc/validate`, () => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: [] })));
    expect(screen.getByText('审阅意见：补充约束')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '校验' }));
    expect(await screen.findByText('校验通过')).toBeInTheDocument();
  });

  it('in_review intent: 批准（G1） sends approve and applies the returned flow; checklist shown', async () => {
    const user = userEvent.setup();
    setup(makeFlow(), 'intent', calls);
    expect(await screen.findByText('☐ 问题是否写清楚了？')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '提交审阅' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '批准（G1）' }));
    await waitFor(() => expect(calls.decisions).toEqual([{ doc: 'intent', action: 'approve' }]));
    // decision handler returns state=spec
    await waitFor(() => expect(screen.getByTestId('flow-state')).toHaveTextContent('起草 spec'));
  });

  it('要求修改 requires a comment before confirming and sends it', async () => {
    const user = userEvent.setup();
    setup(makeFlow(), 'intent', calls);
    await user.click(screen.getByRole('button', { name: '要求修改' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('要求修改 intent')).toBeInTheDocument();
    const ok = within(dialog).getByRole('button', { name: '确定' });
    expect(ok).toBeDisabled();
    await user.type(within(dialog).getByRole('textbox'), '   ');
    expect(ok).toBeDisabled();
    await user.clear(within(dialog).getByRole('textbox'));
    await user.type(within(dialog).getByRole('textbox'), '补充约束章节');
    expect(ok).toBeEnabled();
    await user.click(ok);
    await waitFor(() => expect(calls.decisions).toEqual([{ doc: 'intent', action: 'request_changes', comment: '补充约束章节' }]));
  });

  it('拒绝 is only offered on intent; reason is optional', async () => {
    const user = userEvent.setup();
    setup(makeFlow(), 'intent', calls);
    await user.click(screen.getByRole('button', { name: '拒绝' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('拒绝 intent')).toBeInTheDocument();
    const ok = within(dialog).getByRole('button', { name: '确定' });
    expect(ok).toBeEnabled();
    await user.click(ok);
    await waitFor(() => expect(calls.decisions).toEqual([{ doc: 'intent', action: 'reject' }]));
  });

  it('spec in review has no 拒绝 button', () => {
    setup(
      makeFlow({ state: 'spec_review', documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'in_review' })] }),
      'spec',
      calls,
    );
    expect(screen.getByRole('button', { name: '批准（G2）' })).toBeEnabled();
    expect(screen.getByRole('button', { name: '要求修改' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '拒绝' })).not.toBeInTheDocument();
  });

  it('G3: merge pre-check failure disables approval and shows the reason', () => {
    setup(
      makeFlow({
        state: 'plan_review',
        documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'approved' }), doc('plan', { status: 'in_review' })],
        mergePrecheck: { canMerge: false, reason: '你的 GitLab Token 没有合并权限（需要 Maintainer）' },
      }),
      'plan',
      calls,
    );
    expect(screen.getByRole('button', { name: '批准（G3）' })).toBeDisabled();
    expect(screen.getByText('你的 GitLab Token 没有合并权限（需要 Maintainer）')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '要求修改' })).toBeEnabled();
  });

  it('G3: approval enabled when pre-check passes', async () => {
    const user = userEvent.setup();
    setup(
      makeFlow({
        state: 'plan_review',
        documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'approved' }), doc('plan', { status: 'in_review' })],
      }),
      'plan',
      calls,
    );
    await user.click(screen.getByRole('button', { name: '批准（G3）' }));
    await waitFor(() => expect(calls.decisions).toEqual([{ doc: 'plan', action: 'approve' }]));
  });

  it('approval is disabled while the AI runs on the stage', () => {
    setup(makeFlow(), 'intent', calls);
    act(() => useWorkbenchStore.setState((s) => ({ stages: { ...s.stages, intent: { ...s.stages.intent, running: true } } })));
    expect(screen.getByRole('button', { name: '批准（G1）' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '要求修改' })).toBeDisabled();
  });

  it('draft failure offers 重试 and 改为人工撰写', async () => {
    const user = userEvent.setup();
    const hits: string[] = [];
    setup(
      makeFlow({
        state: 'spec',
        documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'draft', draftState: 'failed', draftFailures: 2, draftError: 'turn failed: boom' })],
      }),
      'spec',
      calls,
    );
    server.use(
      http.post(`${base}/docs/spec/draft/retry`, () => {
        hits.push('retry');
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: null });
      }),
      http.post(`${base}/docs/spec/draft/manual`, () => {
        hits.push('manual');
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: null });
      }),
    );
    expect(screen.getByText('起草失败（2 次）：turn failed: boom')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '重试' }));
    await waitFor(() => expect(hits).toEqual(['retry']));
    await user.click(screen.getByRole('button', { name: '改为人工撰写' }));
    await waitFor(() => expect(hits).toEqual(['retry', 'manual']));
  });

  it('shows AI drafting progress', () => {
    setup(
      makeFlow({ state: 'spec', documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'draft', draftState: 'running' })] }),
      'spec',
      calls,
    );
    expect(screen.getByText('AI 正在起草 spec…')).toBeInTheDocument();
  });

  it('lists approval records; invalidated ones are struck through', () => {
    const sha = 'a'.repeat(64);
    setup(
      makeFlow({
        state: 'spec_review',
        documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'in_review' })],
        approvals: [
          { id: 1, documentId: 10, gate: 'G1', action: 'request_changes', actor: 1, actorName: 'admin', comment: 'x', blobSha: 'b'.repeat(64), commitSha: null, createdAt: '2026-10-26 01:00:00', invalidatedAt: '2026-10-26 02:00:00' },
          { id: 2, documentId: 10, gate: 'G1', action: 'approve', actor: 1, actorName: 'admin', comment: null, blobSha: sha, commitSha: null, createdAt: '2026-10-26 03:00:00', invalidatedAt: null },
        ],
      }),
      'spec',
      calls,
    );
    expect(screen.getByText('审批记录')).toBeInTheDocument();
    const ok = screen.getByText('G1 批准 · admin · 2026-10-26 03:00:00 · aaaaaaaa');
    expect(ok).not.toHaveStyle({ textDecoration: 'line-through' });
    const old = screen.getByText('G1 要求修改 · admin · 2026-10-26 01:00:00 · bbbbbbbb');
    expect(old).toHaveStyle({ textDecoration: 'line-through' });
  });

  it('todo state: shows 文档已合入 and the revert entry, no gate buttons', () => {
    setup(
      makeFlow({
        state: 'todo',
        documents: [doc('intent', { status: 'merged' }), doc('spec', { status: 'merged' }), doc('plan', { status: 'merged' })],
      }),
      'plan',
      calls,
    );
    expect(screen.getByTestId('flow-state')).toHaveTextContent('文档已合入，进入实现');
    expect(screen.getByRole('button', { name: '退回文档阶段' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /批准/ })).not.toBeInTheDocument();
    expect(screen.getByTestId('doc-chip-plan')).toHaveTextContent('plan · 已合入');
  });

  it('pending merge with error offers 重试合并 gated by pre-check', () => {
    setup(
      makeFlow({
        state: 'plan_approved_pending_merge',
        flow: { ...makeFlow().flow, mergeError: '405 Method Not Allowed' },
        documents: [doc('intent', { status: 'approved' }), doc('spec', { status: 'approved' }), doc('plan', { status: 'approved' })],
      }),
      'plan',
      calls,
    );
    expect(screen.getByText('已批准，待合并：405 Method Not Allowed')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '重试合并' })).toBeEnabled();
  });

  it('server error on decision is surfaced as a notice', async () => {
    const user = userEvent.setup();
    setup(makeFlow(), 'intent', calls);
    server.use(
      http.post(`${base}/docs/:doc/decision`, () =>
        HttpResponse.json({ success: false, retCode: 'BIZ_003', retMsg: '文档已变更，请刷新后重新审阅' }, { status: 409 }),
      ),
    );
    await user.click(screen.getByRole('button', { name: '批准（G1）' }));
    await waitFor(() =>
      expect(useWorkbenchStore.getState().notices.map((n) => n.message)).toContain('文档已变更，请刷新后重新审阅'),
    );
  });
});
