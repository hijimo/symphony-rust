import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../theme';
import { server } from '../../test/mocks/server';
import AdminOrgConfig from '../AdminOrgConfig';

const SHA = 'c'.repeat(40);
const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });

function status(over: Record<string, unknown> = {}) {
  return {
    enabled: true,
    url: 'https://git.local/org/rules.git',
    ref: 'main',
    intervalSecs: 300,
    currentSha: SHA,
    lastSyncAt: '2026-11-16T03:00:00Z',
    lastError: null,
    pins: [{ projectId: 2, pin: 'v1', sha: 'd'.repeat(40), error: null }],
    versions: [SHA],
    droppedConfig: ['mcp_servers', 'approval_policy'],
    ...over,
  };
}

function renderPage() {
  render(
    <ThemeProvider theme={theme}>
      <AdminOrgConfig />
    </ThemeProvider>,
  );
}

describe('AdminOrgConfig', () => {
  let configPuts: unknown[];
  let syncs: number;
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    configPuts = [];
    syncs = 0;
    server.use(
      http.get('*/api/admin/org-config', () => ok(status())),
      http.post('*/api/admin/org-config/sync', () => {
        syncs += 1;
        return ok(status({ lastError: '同步失败：authentication failed' }));
      }),
      http.put('*/api/admin/config', async ({ request }) => {
        configPuts.push(await request.json());
        return ok([]);
      }),
    );
  });

  it('shows the sync status, pins and dropped config.toml entries', async () => {
    renderPage();
    expect(await screen.findByDisplayValue('https://git.local/org/rules.git')).toBeInTheDocument();
    expect(screen.getByDisplayValue('main')).toBeInTheDocument();
    expect(screen.getByTestId('org-current')).toHaveTextContent(SHA.slice(0, 8));
    expect(screen.getByText('mcp_servers')).toBeInTheDocument();
    expect(screen.getByText('approval_policy')).toBeInTheDocument();
    expect(screen.getByText('项目 #2')).toBeInTheDocument();
  });

  it('saves the repository settings and syncs now', async () => {
    const user = userEvent.setup();
    renderPage();
    const url = await screen.findByLabelText('仓库地址');
    await user.clear(url);
    await user.type(url, 'git@git.local:org/rules.git');
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() =>
      expect(configPuts).toEqual([
        {
          configs: [
            { key: 'org_config.url', value: 'git@git.local:org/rules.git' },
            { key: 'org_config.ref', value: 'main' },
          ],
        },
      ]),
    );
    await user.click(screen.getByRole('button', { name: '立即同步' }));
    expect(await screen.findByText('同步失败：authentication failed')).toBeInTheDocument();
    expect(syncs).toBe(1);
  });

  it('disables with an empty address', async () => {
    const user = userEvent.setup();
    server.use(http.get('*/api/admin/org-config', () => ok(status({ enabled: false, url: null, currentSha: null, droppedConfig: [], pins: [] }))));
    renderPage();
    expect(await screen.findByText('未启用：填写仓库地址后保存即可启用')).toBeInTheDocument();
    await user.type(screen.getByLabelText('仓库地址'), 'x');
    await user.clear(screen.getByLabelText('仓库地址'));
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() =>
      expect(configPuts).toEqual([
        {
          configs: [
            { key: 'org_config.url', value: '-' },
            { key: 'org_config.ref', value: 'main' },
          ],
        },
      ]),
    );
  });
});
