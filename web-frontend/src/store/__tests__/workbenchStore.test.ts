import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { currentStage, handleMessage, reduceAgentEvent, useWorkbenchStore } from '../workbenchStore';
import { makeFlow } from '../../test/workbenchFixtures';

const empty = { chat: [], running: false, pending: [], lastDiff: null };

describe('reduceAgentEvent', () => {
  it('accumulates deltas and finishes the turn', () => {
    let s = reduceAgentEvent(empty, { type: 'user_message', text: 'hi', mode: 'interactive' });
    expect(s.running).toBe(true);
    s = reduceAgentEvent(s, { type: 'message_delta', itemId: 'm1', delta: 'Hel' });
    s = reduceAgentEvent(s, { type: 'message_delta', itemId: 'm1', delta: 'lo' });
    expect(s.chat.filter((c) => c.role === 'assistant')).toHaveLength(1);
    expect(s.chat[1].text).toBe('Hello');
    s = reduceAgentEvent(s, { type: 'item', phase: 'started', item: { id: 'c1', kind: 'command', command: 'ls', status: 'inProgress' } });
    s = reduceAgentEvent(s, { type: 'command_output', itemId: 'c1', delta: 'a.md' });
    s = reduceAgentEvent(s, { type: 'item', phase: 'completed', item: { id: 'c1', kind: 'command', command: 'ls', status: 'completed', exitCode: 0 } });
    const cmd = s.chat.find((c) => c.role === 'command')!;
    expect(cmd.meta?.status).toBe('completed');
    expect(cmd.meta?.output).toBe('a.md');
    s = reduceAgentEvent(s, { type: 'turn_diff', diff: '+x', baseSha: 'abc', files: ['docs/changes/1/spec.md'] } as never);
    expect(s.lastDiff?.files).toEqual(['docs/changes/1/spec.md']);
    s = reduceAgentEvent(s, { type: 'turn_completed', status: 'completed', error: null });
    expect(s.running).toBe(false);
  });

  it('tracks and resolves pending approvals and questions', () => {
    let s = reduceAgentEvent(empty, { type: 'approval_request', requestId: 'r1', kind: 'command', command: 'rm', cwd: null, reason: null });
    s = reduceAgentEvent(s, { type: 'user_input_request', requestId: 'r2', questions: [{ id: 'q', header: 'H', question: 'Q?' }] });
    expect(s.pending.map((p) => p.requestId)).toEqual(['r1', 'r2']);
    s = reduceAgentEvent(s, { type: 'request_resolved', requestId: 'r1' });
    expect(s.pending.map((p) => p.requestId)).toEqual(['r2']);
    s = reduceAgentEvent(s, { type: 'turn_completed', status: 'failed', error: 'boom' });
    expect(s.pending).toEqual([]);
    expect(s.chat[s.chat.length - 1].text).toContain('boom');
  });
});

describe('currentStage', () => {
  it('maps flow state to the stage to focus', () => {
    expect(currentStage(makeFlow({ state: 'intent' }))).toBe('intent');
    expect(currentStage(makeFlow({ state: 'spec_review' }))).toBe('spec');
    expect(currentStage(makeFlow({ state: 'todo' }))).toBe('intent');
    const plan = makeFlow({ state: 'plan_review' });
    plan.documents.push({ ...plan.documents[0], id: 11, docType: 'plan' });
    expect(currentStage(plan)).toBe('plan');
  });
});

describe('handleMessage', () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      stages: { intent: { ...empty }, spec: { ...empty }, plan: { ...empty } },
      notices: [],
    });
  });
  afterEach(() => useWorkbenchStore.getState().dispose());

  it('replays history and applies agent / state / notice messages', () => {
    handleMessage({ type: 'history', stage: 'spec', events: [{ type: 'user_message', text: 'draft', mode: 'non_interactive' }, { type: 'turn_completed', status: 'completed', error: null }] });
    expect(useWorkbenchStore.getState().stages.spec.chat).toHaveLength(2);
    handleMessage({ type: 'agent_state', stage: 'plan', running: true });
    expect(useWorkbenchStore.getState().stages.plan.running).toBe(true);
    handleMessage({ type: 'agent', stage: 'plan', event: { type: 'message_delta', itemId: 'x', delta: 'ok' } });
    expect(useWorkbenchStore.getState().stages.plan.chat[0].text).toBe('ok');
    handleMessage({ type: 'notice', level: 'warning', message: 'label 同步失败' });
    expect(useWorkbenchStore.getState().notices[0].message).toBe('label 同步失败');
  });
});
