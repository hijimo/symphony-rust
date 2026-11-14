import { test, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { SDLC_E2E, gitlabUrl } from './env';

/**
 * AI-native SDLC P2 critical flows against the real web-platform
 * (docs/方案/ai-native-sdlc-p2-design.md):
 *   B6 gate approvers (settings → member blocked → approver decides),
 *   A6 drafting token usage in the workbench and doc sessions on the
 *      concurrency page,
 *   A4 label/document mismatch on the stage board,
 *   B5 metrics bar,
 *   A7 document template page (save through an MR, trial draft).
 *
 * Runs after sdlc-flow.spec.ts on the same stack; it reuses the project of
 * that suite or creates it when run alone.
 */

const slug = `${SDLC_E2E.ns}/${SDLC_E2E.repo}`;
const PASSWORD = 'pw-e2e-123456';
const INTENT = [
  '# Intent: 导出报表',
  '',
  '## Problem',
  '财务每月手工汇总理赔数据',
  '',
  '## Proposed outcome',
  '门户一键导出理赔报表',
  '',
  '## Affected users and systems',
  '财务、门户',
  '',
  '## Constraints',
  '不导出 PII',
  '',
  '## Open questions',
  '无',
  '',
].join('\n');

interface GlState {
  issues: Record<string, Record<string, { iid: number; labels: string[] }>>;
  mrs: Record<string, Record<string, { iid: number; source: string; target: string; state: string }>>;
}

async function glState(): Promise<GlState> {
  return (await (await fetch(`${gitlabUrl()}/__state`)).json()) as GlState;
}

async function login(page: Page, username: string, password: string) {
  await page.goto('/login');
  await page.getByLabel('用户名').fill(username);
  await page.getByPlaceholder('请输入密码').fill(password);
  await page.getByRole('button', { name: '登录' }).click();
  await page.waitForURL(/\/(admin\/users|settings)$/, { timeout: 15_000 });
}

async function api<T = unknown>(page: Page, method: string, path: string, body?: unknown): Promise<T> {
  const token = await page.evaluate(() => localStorage.getItem('token'));
  const r = await page.request.fetch(`/api${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}` },
    data: body,
  });
  const v = await r.json();
  expect(r.ok(), `${method} ${path}: ${JSON.stringify(v)}`).toBe(true);
  return v.data as T;
}

/** Login is rate-limited per user: log in once, then reuse the session. */
const sessions: Record<string, Awaited<ReturnType<BrowserContext['storageState']>>> = {};
const passwords: Record<string, string> = { admin: 'admin123', bob: PASSWORD, carol: PASSWORD };

async function as(browser: Browser, who: string): Promise<Page> {
  const ctx = await browser.newContext(sessions[who] ? { storageState: sessions[who] } : {});
  await ctx.addInitScript(() => {
    (window as unknown as { __PLAIN_EDITOR__?: boolean }).__PLAIN_EDITOR__ = true;
  });
  const page = await ctx.newPage();
  if (!sessions[who]) {
    await login(page, who, passwords[who]);
    sessions[who] = await ctx.storageState();
  }
  return page;
}

let projectId = 0;
const users: Record<string, number> = {};
let gatedIid = 0;

test.describe.serial('AI-native SDLC P2', () => {
  test('setup: project, members bob and carol', async ({ browser }) => {
    const page = await as(browser, 'admin');
    // Token / host are configured by sdlc-flow.spec.ts; configure when run alone.
    await page.goto('/settings');
    const configured = page.getByLabel('GitLab Token 状态：已配置');
    const tokenInput = page.getByPlaceholder('请输入 GitLab Personal Access Token');
    await expect(configured.or(tokenInput)).toBeVisible();
    if (!(await configured.isVisible())) {
      await tokenInput.fill(SDLC_E2E.gitlabToken);
      await page.getByLabel('GitLab Host').fill(gitlabUrl());
      await page.getByRole('button', { name: '保存 Token 配置' }).click();
      await expect(page.getByText('Token 配置已保存')).toBeVisible();
    }

    const list = await api<{ records: { id: number; namespace: string; repoName: string }[] }>(page, 'GET', '/projects?page_size=100');
    const existing = list.records.find((p) => `${p.namespace}/${p.repoName}` === slug);
    projectId = existing
      ? existing.id
      : (await api<{ id: number }>(page, 'POST', '/projects', { git_url: `${gitlabUrl()}/${slug}` })).id;

    for (const name of ['bob', 'carol']) {
      await api(page, 'POST', '/admin/users', { username: name, password: PASSWORD, display_name: name, role: 'user' });
      const all = await api<{ records: { id: number; username: string }[] }>(page, 'GET', '/admin/users?pageSize=100');
      users[name] = all.records.find((u) => u.username === name)!.id;
      await api(page, 'POST', `/projects/${projectId}/members`, { user_id: users[name], role: 'member' });
    }
  });

  test('B6 + A6: only the G1 approver decides; drafting tokens shown', async ({ browser }) => {
    test.skip(!projectId, 'setup failed');
    let page = await as(browser, 'admin');

    // Owner/admin configures G1 → carol in project settings.
    await page.goto(`/projects/${projectId}/settings`);
    await page.getByRole('checkbox', { name: 'carol 可审批 G1' }).check();
    await expect(page.getByText('G1 · intent：仅 carol')).toBeVisible();
    await page.getByRole('button', { name: '保存审批人' }).click();
    await expect(page.getByText('已保存')).toBeVisible();

    const started = await api<{ flow: { issueIid: number } }>(page, 'POST', `/projects/${projectId}/changes`, {
      track: 'full',
      title: '导出报表',
      initialContent: INTENT,
    });
    gatedIid = started.flow.issueIid;
    await api(page, 'POST', `/projects/${projectId}/changes/${gatedIid}/docs/intent/submit`, {});

    // bob (member, not on the list) cannot decide G1.
    page = await as(browser, 'bob');
    await page.goto(`/projects/${projectId}/changes/${gatedIid}`);
    await expect(page.getByTestId('flow-state')).toHaveText('待审 intent（G1）');
    await expect(page.getByRole('button', { name: '批准（G1）' })).toBeDisabled();
    await expect(page.getByRole('button', { name: '要求修改' })).toBeDisabled();
    await expect(page.getByText('仅 carol 可批准')).toBeVisible();

    // carol approves; the AI drafts spec and reports token usage.
    page = await as(browser, 'carol');
    await page.goto(`/projects/${projectId}/changes/${gatedIid}`);
    const approve = page.getByRole('button', { name: '批准（G1）' });
    await expect(approve).toBeEnabled();
    await approve.click();
    await expect(page.getByTestId('flow-state')).toHaveText('待审 spec（G2）', { timeout: 60_000 });
    await page.getByTestId('doc-chip-spec').click();
    await expect(page.getByTestId('doc-token-usage')).toHaveText(
      'AI 用量 1,234 tokens（输入 1,000 · 缓存 200 · 输出 234）',
    );

    // Drafting sessions are listed separately on the concurrency page.
    page = await as(browser, 'admin');
    await page.goto('/admin/concurrency');
    await expect(page.getByTestId('doc-sessions')).toContainText('文档起草会话');
    await expect(page.getByTestId('doc-sessions')).toContainText(/\d+ \/ \d+/);
  });

  test('A4: a hand-moved Todo label is flagged on the stage card', async ({ browser }) => {
    test.skip(!gatedIid, 'B6 flow not created');
    const page = await as(browser, 'admin');
    const r = await fetch(`${gitlabUrl()}/api/v4/projects/${encodeURIComponent(slug)}/issues/${gatedIid}`, {
      method: 'PUT',
      headers: { 'PRIVATE-TOKEN': SDLC_E2E.gitlabToken, 'Content-Type': 'application/json' },
      body: JSON.stringify({ add_labels: 'Todo' }),
    });
    expect(r.ok).toBe(true);
    await page.goto(`/projects/${projectId}/kanban`);
    const card = page.getByTestId(`stage-card-${gatedIid}`);
    const icon = card.getByLabel('label 异常');
    await expect(icon).toBeVisible();
    await icon.hover();
    await expect(page.getByRole('tooltip')).toContainText('按 track:fast 处理');
  });

  test('B5: metrics bar on the project board', async ({ browser }) => {
    test.skip(!projectId, 'setup failed');
    const page = await as(browser, 'admin');
    await page.goto(`/projects/${projectId}/kanban`);
    await expect(page.getByText('流程度量 · 近 30 天')).toBeVisible();
    await expect(page.getByTestId('metric-intent')).toContainText(/\d+%（\d+\/\d+）/);
    await expect(page.getByTestId('metric-cycle')).toBeVisible();
    await expect(page.getByTestId('metric-rework')).toContainText(/\d+ 次 · \d+ 个变更/);
    await expect(page.getByTestId('metric-tokens')).not.toHaveText(/起草 token0$/);
    await page.getByRole('button', { name: '收起度量' }).click();
    await expect(page.getByTestId('metric-intent')).toBeHidden();
    await page.reload();
    await expect(page.getByRole('button', { name: '展开度量' })).toBeVisible();
    await page.getByRole('button', { name: '展开度量' }).click();
    await expect(page.getByTestId('metric-intent')).toBeVisible();
  });

  test('A7: edit a template, save through an MR, trial draft', async ({ browser }) => {
    test.skip(!projectId, 'setup failed');
    const page = await as(browser, 'admin');
    await page.goto(`/projects/${projectId}/settings`);
    await page.getByRole('tab', { name: '文档模板' }).click();
    await page.getByRole('button', { name: 'plan', exact: true }).click();
    await expect(page.getByTestId('template-source')).toHaveText('内置模板');
    const editor = page.getByLabel('模板内容');
    await expect(editor).toHaveValue(/^---\nsymphony_template:/);
    const text = await editor.inputValue();
    await editor.fill(text.replace('## Risks', '## Rollback\n\n## Risks'));

    await page.getByRole('button', { name: '保存（创建 MR）' }).click();
    const alert = page.getByRole('alert').filter({ hasText: '已创建 MR' });
    await expect(alert).toBeVisible({ timeout: 60_000 });
    const gl = await glState();
    const mr = Object.values(gl.mrs[slug]).find((m) => m.source.startsWith('symphony/doc-templates-'));
    expect(mr, 'template MR opened').toBeTruthy();
    expect(mr!.target).toBe('main');
    await expect(alert.getByRole('link', { name: `!${mr!.iid}` })).toBeVisible();

    await page.getByLabel('示例需求').fill('财务需要一键导出理赔报表');
    await page.getByRole('button', { name: '试生成' }).click();
    const result = page.getByTestId('trial-result');
    await expect(result).toBeVisible({ timeout: 120_000 });
    await expect(result).toContainText('Alternatives considered');
    const after = await glState();
    expect(Object.values(after.mrs[slug]).some((m) => m.source.startsWith('symphony/template-trial-'))).toBe(false);
  });
});
