import { defineConfig, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const backendPort = Number(process.env.SDLC_E2E_BACKEND_PORT ?? 3917);
const gitlabPort = Number(process.env.SDLC_E2E_GITLAB_PORT ?? 18929);
const webPort = Number(process.env.SDLC_E2E_WEB_PORT ?? 5197);

/**
 * SDLC document-gate E2E: real browser + real web-platform, fake GitLab
 * (REST + git smart HTTP), local bare repo, fake codex app-server.
 * Isolated ports and DB; never reuses the dev servers on 3000 / 5177.
 *
 *   npx playwright test -c playwright.sdlc.config.ts
 */
export default defineConfig({
  testDir: './e2e/sdlc',
  testMatch: /.*\.spec\.ts/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['html', { outputFolder: 'playwright-report/sdlc', open: 'never' }]],
  outputDir: 'test-results/sdlc',
  timeout: 180_000,
  expect: { timeout: 20_000 },
  use: {
    baseURL: `http://localhost:${webPort}`,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    ...devices['Desktop Chrome'],
    viewport: { width: 1600, height: 1000 },
  },
  globalTeardown: path.join(__dirname, 'e2e/sdlc/teardown.ts'),
  webServer: [
    {
      command: `node ${path.join(__dirname, 'e2e/sdlc/start-stack.mjs')}`,
      url: `http://127.0.0.1:${backendPort}/health`,
      reuseExistingServer: false,
      timeout: 600_000,
      stdout: 'pipe',
      stderr: 'pipe',
      env: {
        SDLC_E2E_BACKEND_PORT: String(backendPort),
        SDLC_E2E_GITLAB_PORT: String(gitlabPort),
        SDLC_E2E_WEB_PORT: String(webPort),
      },
    },
    {
      command: `npx vite --port ${webPort} --strictPort --config e2e/sdlc/vite.sdlc.config.ts`,
      url: `http://localhost:${webPort}`,
      reuseExistingServer: false,
      timeout: 120_000,
      env: { SDLC_E2E_BACKEND_PORT: String(backendPort) },
    },
  ],
});
