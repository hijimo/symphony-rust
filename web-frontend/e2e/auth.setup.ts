import { test as setup, expect } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const authFile = path.resolve(__dirname, '.auth/admin.json');

setup('authenticate as admin', async ({ page }) => {
  // Navigate to login page
  await page.goto('/login');

  // Fill in admin credentials
  await page.getByLabel('用户名').fill('admin');
  await page.getByPlaceholder('请输入密码').fill('admin123');

  // Submit login form
  await page.getByRole('button', { name: '登录' }).click();

  // Wait for navigation after successful login
  await page.waitForURL('**/admin/users', { timeout: 15000 });

  // Verify we're logged in
  await expect(page.locator('body')).not.toContainText('登录失败');

  // Save storage state (localStorage + cookies)
  await page.context().storageState({ path: authFile });
});
