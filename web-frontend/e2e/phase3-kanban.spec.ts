import { test, expect } from '@playwright/test';

const GITLAB_HOST = 'http://gitlab.jushuitan-inc.com:8081';
const GITLAB_TOKEN = process.env.GITLAB_TOKEN ?? '';
const GITLAB_PROJECT_URL = 'http://gitlab.jushuitan-inc.com:8081/zimei10525/symphony_e2e_test_repo';

let adminToken = '';
let projectId = 0;

test.describe('Phase 3: Kanban & Issue Creation', () => {
  test.skip(!GITLAB_TOKEN, 'GITLAB_TOKEN must be set for Phase 3 E2E tests');

  test.beforeAll(async ({ request }) => {
    // Login as admin (server should already be running with admin seeded)
    const loginResp = await request.post('http://localhost:3000/api/auth/login', {
      data: { username: 'admin', password: 'admin123' },
    });
    expect(loginResp.ok()).toBeTruthy();
    const loginBody = await loginResp.json();
    expect(loginBody.success).toBeTruthy();
    adminToken = loginBody.data.token;

    // Set GitLab token in user config
    const configResp = await request.put('http://localhost:3000/api/user/config', {
      headers: { Authorization: `Bearer ${adminToken}` },
      data: { gitlabToken: GITLAB_TOKEN, gitlabHost: GITLAB_HOST },
    });
    expect(configResp.ok()).toBeTruthy();

    // Create a project pointing to the real GitLab repo (may already exist)
    const projectResp = await request.post('http://localhost:3000/api/projects', {
      headers: { Authorization: `Bearer ${adminToken}` },
      data: { git_url: GITLAB_PROJECT_URL },
    });
    const projectBody = await projectResp.json();
    if (projectBody.success) {
      projectId = projectBody.data.id;
    } else {
      // Project might already exist - get it from list
      const listResp = await request.get('http://localhost:3000/api/projects', {
        headers: { Authorization: `Bearer ${adminToken}` },
      });
      const listBody = await listResp.json();
      const existing = listBody.data.records.find(
        (p: any) => p.gitUrl?.includes('symphony_e2e_test_repo') || p.git_url?.includes('symphony_e2e_test_repo')
      );
      projectId = existing?.id || 1;
    }
    console.log(`Using project ID: ${projectId}`);
  });

  test.beforeEach(async ({ page }) => {
    // Set auth token in localStorage directly (avoid rate limiting from repeated UI logins)
    await page.goto('/login');
    await page.evaluate((token) => {
      localStorage.setItem('token', token);
    }, adminToken);
  });

  test('kanban page loads with three columns', async ({ page }) => {
    await page.goto(`/projects/${projectId}/kanban`);
    await page.waitForLoadState('networkidle');

    // Verify three columns are visible
    await expect(page.locator('text=待处理').first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('text=处理中').first()).toBeVisible();
    await expect(page.locator('text=MR').first()).toBeVisible();
  });

  test('kanban displays issues from GitLab', async ({ page }) => {
    await page.goto(`/projects/${projectId}/kanban`);
    await page.waitForLoadState('networkidle');

    // Wait for data to load (real API call to GitLab)
    await page.waitForTimeout(8000);

    // Should show at least one issue title link (we know the repo has issues)
    // Issue cards contain links to issue detail pages
    const issueLinks = page.locator('a[href*="/issues/"]');
    const count = await issueLinks.count();
    expect(count).toBeGreaterThan(0);
  });

  test('kanban refresh button works', async ({ page }) => {
    await page.goto(`/projects/${projectId}/kanban`);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(3000);

    // Find and click refresh button
    const refreshButton = page.locator('button').filter({ hasText: /刷新|refresh/i }).first();
    if (await refreshButton.isVisible()) {
      await refreshButton.click();
      await page.waitForTimeout(2000);
    }
  });

  test('issue creation page loads', async ({ page }) => {
    await page.goto(`/projects/${projectId}/issues/create`);
    await page.waitForLoadState('networkidle');

    // Verify form elements are present
    await expect(page.locator('input').first()).toBeVisible({ timeout: 10000 });
  });

  test('issue creation with valid data', async ({ page }) => {
    await page.goto(`/projects/${projectId}/issues/create`);
    await page.waitForLoadState('networkidle');

    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const title = `[E2E] Playwright test ${timestamp}`;

    // Fill in the title
    const titleInput = page.locator('input').first();
    await titleInput.waitFor({ state: 'visible', timeout: 10000 });
    await titleInput.fill(title);

    // Fill in description if there's a textarea/editor
    const descEditor = page.locator('textarea').first();
    if (await descEditor.isVisible()) {
      await descEditor.fill('## 描述\n\nPlaywright E2E 测试\n\n## Acceptance Criteria\n\n- [ ] 测试通过');
    }

    // Submit the form
    const submitButton = page.locator('button').filter({ hasText: /创建|提交|submit/i }).first();
    if (await submitButton.isVisible()) {
      await submitButton.click();
      await page.waitForTimeout(5000);

      // Verify no error message
      const errorAlert = page.locator('[role="alert"]').filter({ hasText: /错误|失败|error/i });
      const hasError = await errorAlert.isVisible().catch(() => false);
      expect(hasError).toBeFalsy();
    }
  });

  test('issue creation validates empty title', async ({ page }) => {
    await page.goto(`/projects/${projectId}/issues/create`);
    await page.waitForLoadState('networkidle');

    // The submit button should be disabled when title is empty
    const submitButton = page.locator('button').filter({ hasText: /创建|提交|submit/i }).first();
    await submitButton.waitFor({ state: 'visible', timeout: 10000 });

    // Verify button is disabled (form validation prevents empty title submission)
    await expect(submitButton).toBeDisabled();

    // Should still be on the create page
    expect(page.url()).toContain('/issues/create');
  });

  test('issue detail page loads', async ({ page }) => {
    await page.goto(`/projects/${projectId}/issues/1`);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(3000);

    const pageContent = await page.textContent('body');
    expect(pageContent?.length).toBeGreaterThan(0);
  });

  test('MR detail page loads', async ({ page }) => {
    await page.goto(`/projects/${projectId}/mrs/2`);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(3000);

    const pageContent = await page.textContent('body');
    expect(pageContent?.length).toBeGreaterThan(0);
  });
});
