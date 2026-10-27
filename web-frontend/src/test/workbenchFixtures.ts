import { http, HttpResponse } from 'msw';
import type { FlowView, StageKanban } from '../types/changes';

export const PID = 7;
export const IID = 12;

export function makeFlow(over: Partial<FlowView> = {}): FlowView {
  return {
    flow: {
      id: 1,
      projectId: PID,
      issueIid: IID,
      track: 'full',
      revision: 1,
      branch: `symphony/docs-${IID}`,
      docsMrIid: 3,
      docsMrUrl: 'http://gitlab.local/mr/3',
      mergeCommitSha: null,
      mergeError: null,
      baseSha: 'abc1234',
      closed: false,
      createdBy: 1,
      createdAt: '2026-10-26 00:00:00',
      updatedAt: '2026-10-26 00:00:00',
    },
    state: 'intent_review',
    label: 'doc:intent-review',
    documents: [
      {
        id: 10,
        flowId: 1,
        docType: 'intent',
        status: 'in_review',
        path: `docs/changes/${IID}/intent.md`,
        currentBlobSha: 'f'.repeat(64),
        templateRef: 'intent@1',
        draftState: 'idle',
        draftFailures: 0,
        draftError: null,
        lastFeedback: null,
        updatedAt: '2026-10-26 00:00:00',
      },
    ],
    approvals: [],
    writableDir: `docs/changes/${IID}`,
    multiRepo: false,
    primaryDir: '',
    runningStage: null,
    mergePrecheck: { canMerge: true, reason: null },
    ...over,
  };
}

export const INTENT_TEXT = `---\nsymphony:\n  doc: intent\n---\n# Intent: X\n\n## Problem\nusers cannot X\n\n| a | b |\n|---|---|\n| 1 | 2 |\n`;

export interface WbCalls {
  writes: unknown[];
  decisions: unknown[];
  submits: string[];
}

export function workbenchHandlers(flow: FlowView, calls: WbCalls, files: Record<string, string> = {}) {
  const all: Record<string, string> = {
    [`docs/changes/${IID}/intent.md`]: INTENT_TEXT,
    'src/main.rs': 'fn main() {}\n',
    ...files,
  };
  let version = 1;
  const base = `*/api/projects/${PID}/changes/${IID}`;
  return [
    http.get(base, () => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: flow })),
    http.post(`${base}/workspace`, () =>
      HttpResponse.json({
        success: true,
        retCode: '0',
        retMsg: 'ok',
        data: { context: { baseSha: 'abc', behindMain: true, missing: [] }, writableDir: flow.writableDir, multiRepo: false, primaryDir: '' },
      }),
    ),
    http.get(`${base}/tree`, ({ request }) => {
      const path = new URL(request.url).searchParams.get('path') ?? '';
      const data =
        path === ''
          ? [
              { name: 'docs', path: 'docs', kind: 'dir', size: null, writable: false },
              { name: 'src', path: 'src', kind: 'dir', size: null, writable: false },
            ]
          : path === 'src'
            ? [{ name: 'main.rs', path: 'src/main.rs', kind: 'file', size: 12, writable: false }]
            : [];
      return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });
    }),
    http.get(`${base}/file`, ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path') ?? '';
      if (url.searchParams.get('rev')) {
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { path, content: 'old\n' } });
      }
      if (!(path in all)) return HttpResponse.json({ success: false, retCode: 'BIZ_002', retMsg: '文件不存在' }, { status: 404 });
      return HttpResponse.json({
        success: true,
        retCode: '0',
        retMsg: 'ok',
        data: { path, content: all[path], version: `v${version}`, size: all[path].length, binary: false, tooLarge: false, writable: path.startsWith(flow.writableDir) },
      });
    }),
    http.put(`${base}/file`, async ({ request }) => {
      const body = (await request.json()) as { path: string; content: string; baseVersion: string; commit?: boolean };
      calls.writes.push(body);
      if (body.baseVersion !== `v${version}`) {
        return HttpResponse.json({ success: false, retCode: 'BIZ_003', retMsg: '文件已被修改，请查看差异后再保存' }, { status: 409 });
      }
      all[body.path] = body.content;
      version += 1;
      return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { version: `v${version}`, commitSha: body.commit ? 'deadbeefcafe' : null } });
    }),
    http.get(`${base}/search`, () =>
      HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: [{ path: 'src/main.rs', line: 1, text: 'fn main() {}' }] }),
    ),
    http.get(`${base}/templates`, () =>
      HttpResponse.json({
        success: true,
        retCode: '0',
        retMsg: 'ok',
        data: [{ docType: 'intent', source: 'builtin', templateRef: 'intent@1', skeleton: '', prompt: '', checklist: ['问题是否写清楚了？'] }],
      }),
    ),
    http.post(`${base}/docs/:doc/validate`, () =>
      HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: [{ section: 'Constraints', message: '章节 ## Constraints 不能为空' }] }),
    ),
    http.post(`${base}/docs/:doc/submit`, ({ params }) => {
      calls.submits.push(String(params.doc));
      return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: flow });
    }),
    http.post(`${base}/docs/:doc/decision`, async ({ request, params }) => {
      calls.decisions.push({ doc: params.doc, ...((await request.json()) as object) });
      return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { ...flow, state: 'spec' } });
    }),
    http.post(`${base}/ws-ticket`, () => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: { ticket: 't1' } })),
  ];
}

export function stageKanban(): StageKanban {
  const card = (iid: number, column: StageKanban['columns'][number]['key'], subState: string, extra = {}) => ({
    iid,
    title: `Issue ${iid}`,
    state: 'opened',
    labels: [],
    author: { username: 'a', display_name: null, avatar_url: null },
    updatedAt: '2026-10-26',
    webUrl: '',
    placement: { column, subState, anomaly: false, waitingHuman: subState.startsWith('待审') },
    track: null,
    flow: null,
    ...extra,
  });
  return {
    platform: 'gitlab',
    prError: null,
    columns: [
      { key: 'plan', title: '计划', hint: 'intent', totalCount: 2, mergeRequests: [], issues: [card(1, 'plan', '待启动'), card(2, 'plan', '待审 intent', { track: 'full', flow: { state: 'intent_review', track: 'full', docsMrUrl: 'http://mr', mergeError: null, documents: [{ docType: 'intent', status: 'in_review', draftState: 'idle' }] } })] },
      { key: 'design', title: '设计', hint: 'spec', totalCount: 0, mergeRequests: [], issues: [] },
      { key: 'build', title: '构建与测试', hint: 'plan → 实现 → 测试', totalCount: 1, mergeRequests: [], issues: [card(3, 'build', '实现中', { placement: { column: 'build', subState: '实现中', anomaly: true, waitingHuman: false } })] },
      { key: 'deploy', title: '部署', hint: '代码审查与合并', totalCount: 0, issues: [], mergeRequests: [{ iid: 9, title: 'feat', web_url: '', source_branch: 'symphony/app-3', ci_status: 'success', author: { username: 'a' } }] },
      { key: 'maintain', title: '运维', hint: '近期完成', totalCount: 0, mergeRequests: [], issues: [] },
    ],
  };
}

/** Minimal WebSocket mock capturing sent frames. */
export class MockSocket {
  static instances: MockSocket[] = [];
  static OPEN = 1;
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((m: { data: string }) => void) | null = null;
  constructor(public url: string) {
    MockSocket.instances.push(this);
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.();
    }, 0);
  }
  send(d: string) {
    this.sent.push(d);
  }
  close() {
    this.readyState = 3;
  }
  emit(v: unknown) {
    this.onmessage?.({ data: JSON.stringify(v) });
  }
}
