import { defineConfig, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: 'html',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:5177',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'setup',
      testMatch: /auth\.setup\.ts/,
    },
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        storageState: path.resolve(__dirname, 'e2e/.auth/admin.json'),
      },
      dependencies: ['setup'],
    },
  ],
  webServer: [
    {
      command: 'cd .. && JWT_SECRET=dev-secret-key-at-least-32-chars-long ENCRYPTION_KEY=MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY= ADMIN_INIT_PASSWORD=admin123 cargo run -p web-platform',
      url: 'http://localhost:3000/health',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
    {
      command: 'npm run dev -- --port 5177',
      url: 'http://localhost:5177',
      reuseExistingServer: !process.env.CI,
      timeout: 30000,
    },
  ],
  globalSetup: path.resolve(__dirname, 'e2e/global-setup.ts'),
});
