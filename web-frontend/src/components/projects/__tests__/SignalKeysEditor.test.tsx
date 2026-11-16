import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import SignalKeysEditor from '../SignalKeysEditor';
import type { SignalIngestKey } from '../../../types/signals';

const PID = 6;
const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });
const key = (id: number, over: Partial<SignalIngestKey> = {}): SignalIngestKey => ({
  id,
  projectId: PID,
  source: 'generic',
  name: `key ${id}`,
  createdBy: 1,
  createdAt: '2026-11-14 00:00:00',
  lastUsedAt: null,
  revokedAt: null,
  ...over,
});

function renderEditor() {
  render(
    <ThemeProvider theme={theme}>
      <SignalKeysEditor projectId={PID} />
    </ThemeProvider>,
  );
}

describe('SignalKeysEditor', () => {
  let keys: SignalIngestKey[];
  let created: unknown[];
  let revoked: string[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    keys = [key(1, { lastUsedAt: '2026-11-14 01:00:00' }), key(2, { source: 'sentry', revokedAt: '2026-11-14 02:00:00' })];
    created = [];
    revoked = [];
    server.use(
      http.get(`*/api/projects/${PID}/signal-keys`, () => ok(keys)),
      http.post(`*/api/projects/${PID}/signal-keys`, async ({ request }) => {
        const body = (await request.json()) as Pick<SignalIngestKey, 'source' | 'name'>;
        created.push(body);
        const k = key(3, body);
        keys = [...keys, k];
        return ok({ ...k, secret: 'sig_plaintext' });
      }),
      http.delete(`*/api/projects/${PID}/signal-keys/:id`, ({ params }) => {
        revoked.push(String(params.id));
        keys = keys.map((k) => (String(k.id) === params.id ? { ...k, revokedAt: 'now' } : k));
        return ok(null);
      }),
    );
  });

  it('lists keys with their source and the ingest URL', async () => {
    renderEditor();
    const row = await screen.findByTestId('signal-key-1');
    expect(within(row).getByText('key 1')).toBeInTheDocument();
    expect(within(row).getByText('generic')).toBeInTheDocument();
    expect(within(screen.getByTestId('signal-key-2')).getByText('已吊销')).toBeInTheDocument();
    expect(screen.getByText(`/api/ingest/${PID}/<来源>`, { exact: false })).toBeInTheDocument();
  });

  it('creates a key and shows the secret once', async () => {
    const user = userEvent.setup();
    renderEditor();
    await screen.findByTestId('signal-key-1');
    await user.type(screen.getByLabelText('密钥名称'), 'grafana');
    await user.click(screen.getByRole('combobox', { name: '来源' }));
    await user.click(await screen.findByRole('option', { name: 'alertmanager' }));
    await user.click(screen.getByRole('button', { name: '生成密钥' }));
    expect(await screen.findByText('sig_plaintext')).toBeInTheDocument();
    expect(screen.getByText(/只显示这一次/)).toBeInTheDocument();
    expect(created).toEqual([{ source: 'alertmanager', name: 'grafana' }]);
    expect(await screen.findByTestId('signal-key-3')).toBeInTheDocument();
  });

  it('revokes an active key', async () => {
    const user = userEvent.setup();
    renderEditor();
    const row = await screen.findByTestId('signal-key-1');
    await user.click(within(row).getByRole('button', { name: '吊销' }));
    await waitFor(() => expect(revoked).toEqual(['1']));
    expect(await within(screen.getByTestId('signal-key-1')).findByText('已吊销')).toBeInTheDocument();
    expect(within(screen.getByTestId('signal-key-2')).queryByRole('button', { name: '吊销' })).not.toBeInTheDocument();
  });

  it('shows the server error', async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`*/api/projects/${PID}/signal-keys`, () =>
        HttpResponse.json({ success: false, retCode: 'AUTH_003', retMsg: '无权限' }, { status: 403 }),
      ),
    );
    renderEditor();
    await screen.findByTestId('signal-key-1');
    await user.type(screen.getByLabelText('密钥名称'), 'x');
    await user.click(screen.getByRole('button', { name: '生成密钥' }));
    expect(await screen.findByText('无权限')).toBeInTheDocument();
  });
});
