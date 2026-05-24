import { test, expect, type Page } from '@playwright/test';

type ProxyMode = 'disabled' | 'inherit_env' | 'manual';

interface NetworkProxyState {
  mode: ProxyMode;
  version: string;
  source: string;
  httpProxy: {
    configured: boolean;
    displayValue: string;
    updatedAt: string | null;
  };
  httpsProxy: {
    configured: boolean;
    displayValue: string;
    updatedAt: string | null;
  };
  allProxy: {
    configured: boolean;
    displayValue: string;
    updatedAt: string | null;
  };
  noProxy: string;
  autoBypassLocal: boolean;
  needsRestartProjectCount: number;
  updatedAt: string | null;
  warnings: unknown[];
}

function apiOk(data: unknown) {
  return {
    success: true,
    retCode: 'SUCCESS',
    retMsg: 'ok',
    data,
  };
}

async function mockAdminConfigApis(page: Page) {
  let proxyState: NetworkProxyState = {
    mode: 'disabled',
    version: 'proxy-v1',
    source: 'database',
    httpProxy: {
      configured: false,
      displayValue: '',
      updatedAt: null,
    },
    httpsProxy: {
      configured: false,
      displayValue: '',
      updatedAt: null,
    },
    allProxy: {
      configured: false,
      displayValue: '',
      updatedAt: null,
    },
    noProxy: 'localhost,127.0.0.1,::1',
    autoBypassLocal: true,
    needsRestartProjectCount: 2,
    updatedAt: '2026-05-01T00:00:00Z',
    warnings: [],
  };

  await page.route('**/api/admin/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;

    if (request.method() === 'GET' && path === '/api/admin/config') {
      await route.fulfill({
        json: apiOk([
          {
            key: 'global_concurrency_limit',
            value: '10',
            description: '全局并发上限',
            updatedAt: '2026-05-01T00:00:00Z',
          },
          {
            key: 'network_proxy.http_url',
            value: 'http://should-not-render',
            description: '历史误写入代理配置',
            updatedAt: '2026-05-01T00:00:00Z',
          },
        ]),
      });
      return;
    }

    if (request.method() === 'GET' && path === '/api/admin/stats') {
      await route.fulfill({
        json: apiOk({
          totalProjects: 3,
          runningServices: 2,
          totalUsers: 1,
          globalConcurrencyLimit: 10,
          globalConcurrencyUsed: 1,
        }),
      });
      return;
    }

    if (request.method() === 'GET' && path === '/api/admin/network-proxy') {
      await route.fulfill({ json: apiOk(proxyState) });
      return;
    }

    if (request.method() === 'PUT' && path === '/api/admin/network-proxy') {
      const body = request.postDataJSON() as {
        mode: ProxyMode;
        httpProxy: { action: 'keep' | 'set' | 'clear' };
        httpsProxy: { action: 'keep' | 'set' | 'clear' };
        allProxy: { action: 'keep' | 'set' | 'clear' };
        noProxy: string;
        autoBypassLocal: boolean;
      };

      proxyState = {
        ...proxyState,
        mode: body.mode,
        version: 'proxy-v2',
        httpProxy:
          body.httpProxy.action === 'set'
            ? {
                configured: true,
                displayValue: 'http://pl***@proxy.internal:8080',
                updatedAt: '2026-05-02T00:00:00Z',
              }
            : body.httpProxy.action === 'clear'
              ? { configured: false, displayValue: '', updatedAt: null }
              : proxyState.httpProxy,
        httpsProxy:
          body.httpsProxy.action === 'set'
            ? {
                configured: true,
                displayValue: 'http://ht***@proxy.internal:8443',
                updatedAt: '2026-05-02T00:00:00Z',
              }
            : body.httpsProxy.action === 'clear'
              ? { configured: false, displayValue: '', updatedAt: null }
              : proxyState.httpsProxy,
        allProxy:
          body.allProxy.action === 'set'
            ? {
                configured: true,
                displayValue: 'socks5://al***@proxy.internal:1080',
                updatedAt: '2026-05-02T00:00:00Z',
              }
            : body.allProxy.action === 'clear'
              ? { configured: false, displayValue: '', updatedAt: null }
              : proxyState.allProxy,
        noProxy: body.noProxy,
        autoBypassLocal: body.autoBypassLocal,
        updatedAt: '2026-05-02T00:00:00Z',
      };

      await route.fulfill({ json: apiOk(proxyState) });
      return;
    }

    if (request.method() === 'POST' && path === '/api/admin/network-proxy/test') {
      await route.fulfill({
        json: apiOk({
          status: 'success',
          targetHost: 'github.com',
          proxyUsed: proxyState.mode === 'manual',
          proxySummary: proxyState.mode === 'manual' ? 'HTTP proxy' : 'direct',
          durationMs: 38,
          message: '连接成功',
        }),
      });
      return;
    }

    await route.fallback();
  });
}

test.describe('Network proxy admin config', () => {
  test('admin can save manual or disabled proxy config and test connectivity', async ({ page }) => {
    await mockAdminConfigApis(page);

    await page.goto('/admin/config');
    await page.waitForLoadState('networkidle');

    await expect(page.getByRole('button', { name: '系统配置' })).toBeVisible();
    await expect(page.getByRole('heading', { name: '网络代理' })).toBeVisible();
    await expect(page.getByText('需重启服务：2')).toBeVisible();
    await expect(page.getByRole('table')).toContainText('global_concurrency_limit');
    await expect(page.getByRole('table')).not.toContainText('network_proxy.http_url');

    await expect(page.getByLabel('HTTP 代理')).toBeDisabled();
    await page.getByRole('button', { name: '手动配置' }).click();
    await expect(page.getByLabel('HTTP 代理')).toBeEnabled();

    await page.getByLabel('HTTP 代理').fill('http://plain-secret@proxy.internal:8080');
    await page.getByRole('button', { name: '保存代理' }).click();

    await expect(page.getByText('网络代理配置已保存')).toBeVisible();
    await expect(page.getByLabel('HTTP 代理')).toHaveValue('http://pl***@proxy.internal:8080');

    await page.getByRole('button', { name: '测试连接' }).click();
    await expect(page.getByText(/github\.com：连接成功/)).toBeVisible();

    await page.getByRole('button', { name: '禁用' }).click();
    await page.getByRole('button', { name: '保存代理' }).click();
    await expect(page.getByText('网络代理配置已保存')).toBeVisible();
    await expect(page.getByLabel('HTTP 代理')).toBeDisabled();
  });
});
