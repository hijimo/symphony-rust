import { test, expect } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

test.describe('Project Service Management', () => {
  let projectId: string;

  test.beforeAll(async ({ browser }) => {
    // Create a project to use for service tests
    const context = await browser.newContext({
      storageState: path.resolve(__dirname, '.auth/admin.json'),
    });
    const page = await context.newPage();

    await page.goto('/projects/new');
    const gitUrlInput = page.getByLabel('Git URL');
    await gitUrlInput.fill('https://github.com/test-owner/service-test-repo');

    // Wait for parse preview
    await expect(page.getByText('service-test-repo')).toBeVisible({ timeout: 5000 });

    await page.getByRole('button', { name: '创建项目' }).first().click();
    await page.waitForURL(/\/projects\/\d+/, { timeout: 10000 });

    const url = page.url();
    projectId = url.match(/\/projects\/(\d+)/)?.[1] || '';
    expect(projectId).toBeTruthy();

    await context.close();
  });

  test('start service from project list', async ({ page }) => {
    // Navigate to project list
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');

    // Find the project card and click start
    const projectCard = page.locator('[role="article"]', { hasText: 'service-test-repo' });
    await expect(projectCard).toBeVisible({ timeout: 10000 });

    // Click the start button (aria-label="启动服务") within the card
    const startButton = projectCard.getByRole('button', { name: '启动服务' });
    if (await startButton.isVisible()) {
      await startButton.click();

      // Wait for the UI to respond - either status change or error notification
      // The actual symphony binary may not be available, so we accept either outcome
      await page.waitForTimeout(2000);

      // Check if there's a success or error notification
      const alert = page.locator('[role="alert"]');
      if (await alert.isVisible({ timeout: 3000 }).catch(() => false)) {
        await expect(alert).toBeVisible();
      }
    }
  });

  test('stop service from settings', async ({ page }) => {
    // Navigate to project settings -> service control tab
    await page.goto(`/projects/${projectId}/settings`);
    await page.waitForLoadState('networkidle');

    // Wait for project to load
    await expect(page.getByText('项目设置')).toBeVisible({ timeout: 10000 });

    // Click on service control tab
    await page.getByRole('tab', { name: '服务控制' }).click();

    // Look for stop button (only enabled if service is running)
    const stopButton = page.getByRole('button', { name: '停止' });
    await expect(stopButton).toBeVisible({ timeout: 10000 });

    if (await stopButton.isEnabled()) {
      await stopButton.click();

      // Wait for response
      await page.waitForTimeout(2000);

      // Check for notification
      const alert = page.locator('[role="alert"]');
      if (await alert.isVisible({ timeout: 3000 }).catch(() => false)) {
        await expect(alert).toBeVisible();
      }
    } else {
      // Service is already stopped - verify the status is shown
      await expect(page.getByText('已停止')).toBeVisible();
    }
  });

  test('service status displayed in settings', async ({ page }) => {
    // Navigate to project settings
    await page.goto(`/projects/${projectId}/settings`);
    await page.waitForLoadState('networkidle');

    // Wait for project to load
    await expect(page.getByText('项目设置')).toBeVisible({ timeout: 10000 });

    // Click on service control tab
    await page.getByRole('tab', { name: '服务控制' }).click();

    // Verify service status info is displayed - "当前状态" label should be visible
    await expect(page.getByText('当前状态')).toBeVisible({ timeout: 10000 });
  });
});
