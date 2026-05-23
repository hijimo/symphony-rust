import { test, expect } from '@playwright/test';

test.describe('Project CRUD', () => {
  test('create a new project', async ({ page }) => {
    // Navigate to project list
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');

    // Click create project button (use first() since empty state also has one)
    await page.getByRole('button', { name: /创建项目/ }).first().click();
    await page.waitForURL('**/projects/new');

    // Enter git URL
    const gitUrlInput = page.getByLabel('Git URL');
    await gitUrlInput.fill('http://gitlab.jushuitan-inc.com:8081/zimei10525/symphony_e2e_test_repo');

    // Wait for parse preview to appear
    await expect(page.getByText('GitLab')).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('zimei10525')).toBeVisible();
    await expect(page.getByText('symphony_e2e_test_repo')).toBeVisible();

    // Submit the form - use the specific submit button in the form
    await page.getByRole('button', { name: '创建项目' }).click();

    // Verify redirect to project page
    await page.waitForURL(/\/projects\/\d+/, { timeout: 10000 });
    expect(page.url()).toMatch(/\/projects\/\d+/);
  });

  test('project appears in list', async ({ page }) => {
    // Navigate to project list (project was created in previous test)
    await page.goto('/projects');
    await page.waitForLoadState('networkidle');

    // Verify the project card is visible
    await expect(page.getByText('symphony_e2e_test_repo').first()).toBeVisible({ timeout: 10000 });

    // Verify status shows "已停止" (new projects start stopped)
    await expect(page.getByText('已停止').first()).toBeVisible();
  });

  test('delete project', async ({ page }) => {
    // First create a project to delete
    await page.goto('/projects/new');
    const gitUrlInput = page.getByLabel('Git URL');
    await gitUrlInput.fill('https://github.com/test-owner/delete-me-repo');

    // Wait for parse preview
    await expect(page.getByText('delete-me-repo')).toBeVisible({ timeout: 5000 });

    await page.getByRole('button', { name: '创建项目' }).click();
    await page.waitForURL(/\/projects\/\d+/, { timeout: 10000 });

    // Extract project ID from URL
    const url = page.url();
    const projectId = url.match(/\/projects\/(\d+)/)?.[1];
    expect(projectId).toBeTruthy();

    // Navigate to project settings
    await page.goto(`/projects/${projectId}/settings`);
    await page.waitForLoadState('networkidle');

    // Wait for project to load
    await expect(page.getByText('项目设置')).toBeVisible({ timeout: 10000 });

    // Click on "危险操作" (Danger Zone) tab
    await page.getByRole('tab', { name: '危险操作' }).click();

    // Click delete button
    await page.getByRole('button', { name: '删除此项目' }).click();

    // Confirm deletion in dialog
    await page.getByRole('button', { name: '确认删除' }).click();

    // Verify redirect to project list
    await page.waitForURL('**/projects', { timeout: 10000 });
    expect(page.url()).toMatch(/\/projects$/);
  });
});
