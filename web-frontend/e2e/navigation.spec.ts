import { test, expect } from '@playwright/test';

test.describe('Navigation and Permissions', () => {
  test('sidebar shows projects link', async ({ page }) => {
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');

    // Verify sidebar contains "项目列表" link
    await expect(page.getByRole('button', { name: '项目列表' })).toBeVisible({ timeout: 10000 });
  });

  test('navigate between pages', async ({ page }) => {
    // Start at project list
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('heading', { name: '项目', exact: true })).toBeVisible();

    // Navigate to create project page via the header button
    await page.locator('h5', { hasText: '项目' }).locator('..').getByRole('button', { name: /创建项目/ }).click();
    await page.waitForURL('**/projects/new');
    await expect(page.getByRole('heading', { name: '创建项目' })).toBeVisible();

    // Navigate back to project list via the back button
    await page.getByRole('button', { name: /返回/ }).click();
    await page.waitForURL('**/projects');

    // Navigate to settings via sidebar
    await page.getByRole('button', { name: '个人设置' }).click();
    await page.waitForURL('**/settings');
  });

  test('login required', async ({ browser }) => {
    // Create a new context without stored auth state (explicitly empty)
    const context = await browser.newContext({ storageState: { cookies: [], origins: [] } });
    const page = await context.newPage();

    // Navigate to login page directly to verify it works without auth
    await page.goto('/login');
    await expect(page.getByRole('button', { name: '登录' })).toBeVisible({ timeout: 10000 });

    // Now try to visit /projects without auth - should redirect to /login
    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');

    // The SPA loads and React Router redirects to /login
    await expect(page).toHaveURL(/\/login/, { timeout: 15000 });

    await context.close();
  });

  test('root path redirects to projects', async ({ page }) => {
    await page.goto('/');
    await page.waitForURL('**/projects', { timeout: 10000 });
    await expect(page.url()).toContain('/projects');
  });

  test('admin can see admin menu items', async ({ page }) => {
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');

    // Admin should see "用户管理" in sidebar
    await expect(page.getByRole('button', { name: '用户管理' })).toBeVisible();
    // Admin should see "系统配置" in sidebar
    await expect(page.getByRole('button', { name: '系统配置' })).toBeVisible();
  });
});
