import { test, expect, type Page } from '@playwright/test';
import { SDLC_E2E, backendUrl, gitlabUrl } from './env';

/**
 * P3 external signals (docs/方案/ai-native-sdlc-p3-design.md §3): an ingest
 * key in project settings → a monitoring system pushes a signal → it shows
 * in 运维 → 生成 intent → the incident intent is drafted and waits at G1.
 */

const slug = `${SDLC_E2E.ns}/${SDLC_E2E.repo}`;

/** Login is limited to 5 attempts per user and minute; earlier suites use admin a lot. */
async function login(page: Page) {
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.goto('/login');
    await page.getByLabel('用户名').fill('admin');
    await page.getByPlaceholder('请输入密码').fill('admin123');
    await page.getByRole('button', { name: '登录' }).click();
    const limited = page.getByText('too many login attempts', { exact: false });
    const outcome = await Promise.race([
      page.waitForURL(/\/(admin\/users|settings)$/, { timeout: 15_000 }).then(() => 'ok' as const),
      limited.waitFor({ timeout: 15_000 }).then(() => 'limited' as const),
    ]);
    if (outcome === 'ok') return;
    await page.waitForTimeout(61_000);
  }
  throw new Error('login failed');
}

async function api<T = unknown>(page: Page, method: string, path: string, body?: unknown): Promise<T> {
  const token = await page.evaluate(() => localStorage.getItem('token'));
  const r = await page.request.fetch(`/api${path}`, { method, headers: { Authorization: `Bearer ${token}` }, data: body });
  const v = await r.json();
  expect(r.ok(), `${method} ${path}: ${JSON.stringify(v)}`).toBe(true);
  return v.data as T;
}

test('external signal → 运维 → incident intent at G1', async ({ page }) => {
  test.setTimeout(300_000);
  await login(page);
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
  const projectId = existing
    ? existing.id
    : (await api<{ id: number }>(page, 'POST', '/projects', { git_url: `${gitlabUrl()}/${slug}` })).id;

  // Ingest key from the project settings page; shown once.
  await page.goto(`/projects/${projectId}/settings`);
  await page.getByRole('tab', { name: '信号接入' }).click();
  await page.getByLabel('密钥名称').fill('grafana');
  await page.getByRole('button', { name: '生成密钥' }).click();
  const secret = (await page.getByText(/^sig_[0-9a-f]+$/).textContent())!.trim();

  // A monitoring system pushes the same alert twice.
  for (let i = 0; i < 2; i++) {
    const r = await fetch(`${backendUrl()}/api/ingest/${projectId}/generic`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${secret}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        title: '结账接口 5xx 激增',
        fingerprint: 'checkout-5xx',
        severity: 'critical',
        message: '5xx ratio 7.2% over 10m',
        url: 'https://grafana.local/d/checkout',
      }),
    });
    expect(r.status).toBe(202);
  }

  await page.goto(`/projects/${projectId}/kanban`);
  const col = page.getByTestId('stage-col-maintain');
  const card = col.locator('[data-testid^="signal-card-"]').filter({ hasText: '结账接口 5xx 激增' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('触发 2 次');
  await expect(card).toContainText('严重');

  await card.getByRole('button', { name: '生成 intent' }).click();
  await page.waitForURL(/\/changes\/\d+$/, { timeout: 120_000 });
  const iid = page.url().match(/\/changes\/(\d+)$/)![1];
  await expect(page.getByTestId('flow-state')).toHaveText('待审 intent（G1）', { timeout: 60_000 });
  await expect(page.getByTestId('markdown-preview')).toContainText('结账接口 5xx，08:00 起');
  await expect(page.getByText('是否需要先止血、由谁负责？', { exact: false })).toBeVisible();

  const gl = (await (await fetch(`${gitlabUrl()}/__state`)).json()) as {
    issues: Record<string, Record<string, { labels: string[]; description: string | null }>>;
  };
  const issue = gl.issues[slug][iid];
  expect(issue.labels).toEqual(expect.arrayContaining(['source:signal', 'track:full', 'doc:intent-review']));
  expect(issue.description).toContain('https://grafana.local/d/checkout');

  await page.goto(`/projects/${projectId}/kanban`);
  await expect(col.locator('[data-testid^="signal-card-"]').filter({ hasText: '结账接口 5xx 激增' })).toHaveCount(0);
  await expect(page.getByTestId(`stage-card-${iid}`)).toBeVisible();
});
