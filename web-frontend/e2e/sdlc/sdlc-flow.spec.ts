import { test, expect, type Page } from '@playwright/test';
import { SDLC_E2E, gitlabUrl } from './env';

/**
 * Full AI-native SDLC document flow against the real web-platform:
 * login → GitLab token/host → project → stage kanban → 新建需求 → workbench
 * → edit/save intent → submit → G1 → AI spec → G2 → AI plan → G3 → merged/Todo,
 * plus an interactive AI chat turn with streamed reply.
 *
 * External dependencies are all local: fake GitLab (REST + git smart HTTP),
 * bare repo, fake codex app-server (see start-stack.mjs).
 */

const INTENT_BODY = [
  '# Intent: 理赔进度',
  '',
  '## Problem',
  '客户在门户里看不到理赔进度',
  '',
  '## Proposed outcome',
  '门户展示理赔进度',
  '',
  '## Affected users and systems',
  '门户、理赔系统',
  '',
  '## Constraints',
  '不新增 PII',
  '',
  '## Open questions',
  '理赔员是否也需要看？',
  '',
].join('\n');

interface GlState {
  issues: Record<string, Record<string, { iid: number; title: string; labels: string[]; state: string; notes: string[] }>>;
  mrs: Record<string, Record<string, { iid: number; title: string; state: string; source: string; target: string; merge_sha: string | null }>>;
  merge_tokens: string[];
}

async function glState(): Promise<GlState> {
  const r = await fetch(`${gitlabUrl()}/__state`);
  return (await r.json()) as GlState;
}

async function gitShow(ref: string, path: string): Promise<string | null> {
  const u = new URL(`${gitlabUrl()}/__git`);
  u.search = new URLSearchParams({ ns: SDLC_E2E.ns, repo: SDLC_E2E.repo, ref, path }).toString();
  const r = await fetch(u);
  if (!r.ok) return null;
  return ((await r.json()) as { content: string }).content;
}

async function login(page: Page) {
  await page.goto('/login');
  await page.getByLabel('用户名').fill('admin');
  await page.getByPlaceholder('请输入密码').fill('admin123');
  await page.getByRole('button', { name: '登录' }).click();
  await page.waitForURL('**/admin/users', { timeout: 15_000 });
}

const slug = `${SDLC_E2E.ns}/${SDLC_E2E.repo}`;
let projectId = '';

test.describe.serial('AI-native SDLC document flow', () => {
  test.beforeEach(async ({ page }, info) => {
    // Plain editor for flow tests; the [monaco] test uses the real
    // (locally bundled) Monaco editor.
    if (info.title.includes('[monaco]')) return;
    await page.addInitScript(() => {
      (window as unknown as { __PLAIN_EDITOR__?: boolean }).__PLAIN_EDITOR__ = true;
    });
  });

  test('setup: login, configure GitLab token and host, create project, stage kanban', async ({ page }) => {
    await login(page);

    await page.goto('/settings');
    await page.getByPlaceholder('请输入 GitLab Personal Access Token').fill(SDLC_E2E.gitlabToken);
    await page.getByLabel('GitLab Host').fill(gitlabUrl());
    await page.getByRole('button', { name: '保存 Token 配置' }).click();
    await expect(page.getByText('Token 配置已保存')).toBeVisible();
    await expect(page.getByLabel('GitLab Token 状态：已配置')).toBeVisible();

    await page.goto('/projects/new');
    await page.getByLabel('Git URL').fill(`${gitlabUrl()}/${slug}`);
    await page.getByRole('button', { name: '创建项目' }).click();
    await page.waitForURL(/\/projects\/\d+\/kanban/, { timeout: 15_000 });
    projectId = page.url().match(/\/projects\/(\d+)\//)![1];

    // Stage view is the default.
    for (const k of ['plan', 'design', 'build', 'deploy', 'maintain']) {
      await expect(page.getByTestId(`stage-col-${k}`)).toBeVisible();
    }
    await expect(page.getByRole('button', { name: '阶段视图' })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByTestId('stage-col-plan')).toContainText('计划');
  });

  test('new change → workbench → edit/save → G1 → AI spec → G2 → AI plan → G3 → merged + Todo', async ({ page }) => {
    test.skip(!projectId, 'setup test did not create a project');
    await login(page);
    await page.goto(`/projects/${projectId}/kanban`);
    await page.getByRole('button', { name: '新建需求' }).click();
    await page.waitForURL(`**/projects/${projectId}/changes/new`);
    await page.getByLabel('想法').fill('客户想在门户里看到理赔进度');
    await page.getByRole('textbox', { name: '标题' }).fill('理赔进度');
    await page.getByRole('button', { name: '创建并进入工作台' }).click();
    await page.waitForURL(/\/changes\/\d+$/, { timeout: 120_000 });
    const iid = page.url().match(/\/changes\/(\d+)$/)![1];
    const dir = `docs/changes/${iid}`;

    // Fake GitLab: issue created with flow labels, draft docs MR opened.
    let gl = await glState();
    const issue0 = gl.issues[slug][iid];
    expect(issue0.title).toBe('理赔进度');
    expect(issue0.labels).toEqual(expect.arrayContaining(['track:full', 'doc:intent']));
    expect(issue0.labels).not.toContain('Todo');
    const mr0 = Object.values(gl.mrs[slug]).find((m) => m.source === `symphony/docs-${iid}`)!;
    expect(mr0.title.startsWith('Draft:')).toBe(true);

    // Workbench: tree + intent preview.
    await expect(page.getByText('需求文档工作台')).toBeVisible();
    await expect(page.getByTestId('tree-docs')).toBeVisible();
    await expect(page.getByTestId('tree-src')).toBeVisible();
    await expect(page.getByTestId('tree-README.md')).toBeVisible();
    await page.getByTestId('tree-docs').click();
    await page.getByTestId('tree-docs/changes').click();
    await page.getByTestId(`tree-${dir}`).click();
    await expect(page.getByTestId(`tree-${dir}/intent.md`)).toBeVisible();
    const preview = page.getByTestId('markdown-preview');
    await expect(preview.getByRole('heading', { name: 'Intent: 理赔进度' })).toBeVisible();
    await expect(preview).toContainText('客户想在门户里看到理赔进度');
    await expect(preview).not.toContainText('symphony:');
    await expect(page.getByTestId('flow-state')).toHaveText('起草 intent');

    // Validation fails on the skeleton (empty sections).
    await page.getByRole('button', { name: '校验' }).click();
    await expect(page.getByTestId('validation-issues')).toContainText('Proposed outcome');

    // Edit the body (keep the system frontmatter) and commit with 保存.
    // Markdown opens in preview by default; switch to 源码 to edit.
    await page.getByRole('button', { name: '源码' }).click();
    const editor = page.getByLabel('编辑器');
    const original = await editor.inputValue();
    expect(original.startsWith('---\n')).toBe(true);
    const fmEnd = original.indexOf('\n---\n', 4) + 5;
    await editor.fill(original.slice(0, fmEnd) + INTENT_BODY);
    await expect(page.getByLabel('未保存')).toBeVisible();
    await page.getByRole('button', { name: '保存' }).click();
    await expect(page.getByText(/已提交 [0-9a-f]{8}/)).toBeVisible();
    await expect(page.getByLabel('未保存')).toHaveCount(0);
    await expect(preview).toContainText('门户展示理赔进度');
    const pushed = await gitShow(`symphony/docs-${iid}`, `${dir}/intent.md`);
    expect(pushed).toContain('门户展示理赔进度');
    expect(pushed!.startsWith('---\nsymphony:')).toBe(true);

    // Submit → G1
    await page.getByRole('button', { name: '校验' }).click();
    await expect(page.getByText('校验通过')).toBeVisible();
    await page.getByRole('button', { name: '提交审阅' }).click();
    await expect(page.getByTestId('flow-state')).toHaveText('待审 intent（G1）');
    await expect(page.getByTestId('doc-chip-intent')).toHaveText('intent · 待审');
    await page.getByRole('button', { name: '批准（G1）' }).click();

    // AI drafts spec in background and auto-submits it.
    await expect(page.getByTestId('flow-state')).toHaveText('待审 spec（G2）', { timeout: 60_000 });
    await page.getByTestId('doc-chip-spec').click();
    await expect(page.getByTestId('doc-chip-spec')).toHaveText('spec · 待审');
    await expect(preview).toContainText('门户新增进度页');
    await page.getByRole('button', { name: '批准（G2）' }).click();

    // AI drafts plan → G3
    await expect(page.getByTestId('flow-state')).toHaveText('待审 plan（G3）', { timeout: 60_000 });
    await page.getByTestId('doc-chip-plan').click();
    await expect(page.getByTestId('doc-chip-plan')).toHaveText('plan · 待审');
    const g3 = page.getByRole('button', { name: '批准（G3）' });
    await expect(g3).toBeEnabled();
    await g3.click();

    await expect(page.getByTestId('flow-state')).toHaveText('文档已合入，进入实现', { timeout: 60_000 });
    await expect(page.getByTestId('doc-chip-plan')).toHaveText('plan · 已合入');
    await expect(page.getByText('审批记录')).toBeVisible();
    await expect(page.getByText(/^G3 批准 · admin/)).toBeVisible();

    // Fake GitLab: MR merged (Ready, no squash, approver token), issue handed off with Todo.
    gl = await glState();
    const mr = gl.mrs[slug][String(mr0.iid)];
    expect(mr.state).toBe('merged');
    expect(mr.title.startsWith('Draft:')).toBe(false);
    expect(mr.merge_sha).toMatch(/^[0-9a-f]{40}$/);
    expect(gl.merge_tokens).toContain(SDLC_E2E.gitlabToken);
    const issue = gl.issues[slug][iid];
    expect(issue.labels).toContain('Todo');
    expect(issue.labels.filter((l) => l.startsWith('doc:'))).toEqual([]);
    expect(issue.notes.some((n) => n.includes('文档已合入主干'))).toBe(true);
    expect(await gitShow('main', `${dir}/plan.md`)).toContain('## Alternatives considered');
    expect(await gitShow('main', `${dir}/intent.md`)).toContain('门户展示理赔进度');

    // Stage kanban reflects the hand-off.
    await page.goto(`/projects/${projectId}/kanban`);
    const card = page.getByTestId(`stage-card-${iid}`);
    await expect(card).toBeVisible();
    await expect(card.getByLabel('文档进度')).toHaveText('I✔S✔P✔');
  });

  test('workbench AI chat streams a reply over WebSocket', async ({ page }) => {
    test.skip(!projectId, 'setup test did not create a project');
    // Regression for the duplicate-WebSocket defect (StrictMode double init).
    await login(page);
    const openSockets = new Set<string>();
    page.on('websocket', (ws) => {
      if (!ws.url().includes('/ws?ticket=')) return;
      openSockets.add(ws.url());
      ws.on('close', () => openSockets.delete(ws.url()));
    });
    await page.goto(`/projects/${projectId}/changes/new`);
    await page.getByRole('textbox', { name: '标题' }).fill('AI 对话');
    await page.getByRole('button', { name: '创建并进入工作台' }).click();
    await page.waitForURL(/\/changes\/\d+$/, { timeout: 120_000 });

    await expect(page.getByTestId('markdown-preview')).toBeVisible();
    // Wait for the WebSocket (sending earlier only shows "AI 连接未就绪").
    await expect(page.getByLabel('AI 已连接')).toBeVisible({ timeout: 30_000 });
    await page.getByLabel('AI 输入').fill('[chat] 帮我看看这份需求');
    await page.getByRole('button', { name: '发送' }).click();

    const chat = page.getByTestId('chat');
    // Streaming: the reply grows delta by delta before the turn completes.
    await expect(chat.getByTestId('assistant-message').first()).toContainText('收到', { timeout: 30_000 });
    await expect(chat.getByText('— 本轮完成 —').first()).toBeVisible({ timeout: 30_000 });
    await expect(page.getByRole('button', { name: '发送' })).toBeVisible();

    // One workbench connection, one user message, one assistant reply.
    expect([...openSockets], 'open workbench WebSockets').toHaveLength(1);
    await expect(chat.getByText('[chat] 帮我看看这份需求')).toHaveCount(1);
    await expect(chat.getByTestId('assistant-message')).toHaveCount(1);
    await expect(chat.getByTestId('assistant-message')).toHaveText('收到，我来看看这份文档。');
    await expect(chat.getByText('— 本轮完成 —')).toHaveCount(1);
  });

  test('[monaco] tab switching never overwrites a document; AI turn can be reverted', async ({ page }) => {
    test.skip(!projectId, 'setup test did not create a project');
    await login(page);
    const puts: { path: string; content: string }[] = [];
    const cdn: string[] = [];
    page.on('request', (r) => {
      if (r.method() === 'PUT' && r.url().includes('/file')) puts.push(JSON.parse(r.postData() ?? '{}'));
      if (r.url().includes('cdn.jsdelivr.net')) cdn.push(r.url());
    });
    await page.goto(`/projects/${projectId}/changes/new`);
    await page.getByRole('textbox', { name: '标题' }).fill('Monaco 标签');
    await page.getByRole('button', { name: '创建并进入工作台' }).click();
    await page.waitForURL(/\/changes\/(\d+)$/, { timeout: 120_000 });
    const iid = Number(/\/changes\/(\d+)$/.exec(page.url())![1]);
    const intentPath = `docs/changes/${iid}/intent.md`;
    const token = await page.evaluate(() => localStorage.getItem('token'));
    const readIntent = async () => {
      const r = await page.request.get(
        `/api/projects/${projectId}/changes/${iid}/file?path=${encodeURIComponent(intentPath)}`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      return ((await r.json()).data?.content ?? '') as string;
    };

    await expect(page.locator('.monaco-editor').first()).toBeVisible({ timeout: 60_000 });
    await page.getByTestId('tree-README.md').click();
    await expect(page.getByRole('tab', { name: /README\.md/ })).toBeVisible();
    await page.getByRole('tab', { name: /intent\.md/ }).click();
    await page.getByRole('tab', { name: /README\.md/ }).click();
    await page.getByRole('tab', { name: /intent\.md/ }).click();
    await page.waitForTimeout(2500); // > autosave debounce
    const bad = puts.filter((p) => p.path === intentPath && p.content.startsWith('# app'));
    expect(bad, 'intent.md must never receive README content').toHaveLength(0);
    expect(await readIntent()).toContain('## Problem');
    expect(cdn, 'Monaco must not be loaded from a CDN').toHaveLength(0);

    await expect(page.getByLabel('AI 已连接')).toBeVisible({ timeout: 30_000 });
    await page.getByLabel('AI 输入').fill(`完善需求，写入 \`${intentPath}\``);
    await page.getByRole('button', { name: '发送' }).click();
    const review = page.getByTestId('turn-review');
    await expect(review).toBeVisible({ timeout: 30_000 });
    await expect(review).not.toContainText('共 0 处');
    await expect.poll(readIntent, { timeout: 20_000 }).toContain('users cannot X');
    await review.getByRole('button', { name: '整轮回退' }).click();
    await expect.poll(async () => (await readIntent()).includes('users cannot X'), { timeout: 20_000 }).toBe(false);
  });
});
