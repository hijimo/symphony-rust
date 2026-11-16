import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import ProjectOrgConfigPanel from '../ProjectOrgConfigPanel';

const PID = 4;
const HEAD = 'a'.repeat(40);
const V1 = 'b'.repeat(40);
const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });

function state(over: Record<string, unknown> = {}) {
  return {
    enabled: true,
    url: 'https://git.local/org/rules.git',
    ref: 'main',
    followedSha: HEAD,
    pin: null,
    effectiveSha: HEAD,
    pinError: null,
    canEdit: true,
    ...over,
  };
}

function renderPanel() {
  render(
    <ThemeProvider theme={theme}>
      <ProjectOrgConfigPanel projectId={PID} />
    </ThemeProvider>,
  );
}

describe('ProjectOrgConfigPanel', () => {
  let puts: unknown[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    puts = [];
  });

  it('says when the organization repository is not in use', async () => {
    server.use(http.get(`*/api/projects/${PID}/org-config`, () => ok(state({ enabled: false, url: null }))));
    renderPanel();
    expect(await screen.findByText('未启用组织规则仓库')).toBeInTheDocument();
  });

  it('shows the effective version and locks to a tag', async () => {
    const user = userEvent.setup();
    server.use(
      http.get(`*/api/projects/${PID}/org-config`, () => ok(state())),
      http.put(`*/api/projects/${PID}/org-config`, async ({ request }) => {
        const body = (await request.json()) as { pin: string | null };
        puts.push(body);
        return ok(state({ pin: body.pin, effectiveSha: body.pin ? V1 : HEAD }));
      }),
    );
    renderPanel();
    expect(await screen.findByText('https://git.local/org/rules.git')).toBeInTheDocument();
    expect(screen.getByTestId('org-effective')).toHaveTextContent(`${HEAD.slice(0, 8)}（跟随 main）`);
    expect(screen.getByText(/重启服务后生效/)).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: '锁定到 tag / commit' }));
    await user.type(screen.getByLabelText('锁定版本'), 'v1');
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(puts).toEqual([{ pin: 'v1' }]));
    expect(await screen.findByTestId('org-effective')).toHaveTextContent(`${V1.slice(0, 8)}（锁定 v1）`);

    await user.click(screen.getByRole('radio', { name: '跟随最新' }));
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(puts).toEqual([{ pin: 'v1' }, { pin: null }]));
  });

  it('shows the pin error and is read-only for members', async () => {
    server.use(
      http.get(`*/api/projects/${PID}/org-config`, () =>
        ok(state({ pin: 'gone', effectiveSha: null, pinError: '找不到锁定的版本 gone，或尚未同步成功', canEdit: false })),
      ),
    );
    renderPanel();
    expect(await screen.findByText('找不到锁定的版本 gone，或尚未同步成功')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled();
    expect(screen.getByRole('radio', { name: '跟随最新' })).toBeDisabled();
    expect(screen.getByText('只有项目 owner 可以修改')).toBeInTheDocument();
  });

  it('shows the server error on save', async () => {
    const user = userEvent.setup();
    server.use(
      http.get(`*/api/projects/${PID}/org-config`, () => ok(state())),
      http.put(`*/api/projects/${PID}/org-config`, () =>
        HttpResponse.json({ success: false, retCode: 'BIZ_001', retMsg: '版本无效：-x' }, { status: 400 }),
      ),
    );
    renderPanel();
    await user.click(await screen.findByRole('radio', { name: '锁定到 tag / commit' }));
    await user.type(screen.getByLabelText('锁定版本'), '-x');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect(await screen.findByText('版本无效：-x')).toBeInTheDocument();
  });
});
