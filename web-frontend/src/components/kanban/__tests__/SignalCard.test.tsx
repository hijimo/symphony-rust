import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import { PID, stageKanban } from '../../../test/workbenchFixtures';
import StageBoard from '../StageBoard';
import type { SignalRecord } from '../../../types/signals';

function signal(id: number, over: Partial<SignalRecord> = {}): SignalRecord {
  return {
    id,
    projectId: PID,
    source: 'sentry',
    fingerprint: `fp-${id}`,
    title: `Signal ${id}`,
    severity: 'critical',
    status: 'new',
    resolved: false,
    firstSeenAt: '2026-11-14 08:00:00',
    lastSeenAt: '2026-11-14 09:00:00',
    count: 3,
    evidence: { message: 'boom', details: { frames: ['a.py:1'] } },
    externalUrl: 'https://sentry.local/issues/1/',
    flowId: null,
    issueIid: null,
    ignoredUntil: null,
    triagedBy: null,
    triagedAt: null,
    ...over,
  };
}

function renderBoard(signals: SignalRecord[], onChanged = vi.fn()) {
  const data = stageKanban();
  data.columns[4] = { ...data.columns[4], signals, signalCount: signals.length };
  render(
    <ThemeProvider theme={theme}>
      <MemoryRouter>
        <StageBoard data={data} projectId={PID} onChanged={onChanged} />
      </MemoryRouter>
    </ThemeProvider>,
  );
  return { onChanged };
}

describe('signal cards in 运维', () => {
  let ignored: unknown[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    ignored = [];
    server.use(
      http.post(`*/api/projects/:pid/signals/:id/ignore`, async ({ request, params }) => {
        ignored.push({ pid: params.pid, id: params.id, ...((await request.json()) as object) });
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: signal(Number(params.id), { status: 'ignored' }) });
      }),
    );
  });

  it('shows source, severity, count, resolved state and the source link', () => {
    renderBoard([signal(1), signal(2, { severity: 'info', resolved: true, source: 'alertmanager', count: 1 })]);
    const col = screen.getByTestId('stage-col-maintain');
    expect(within(col).getByText('待分诊 2')).toBeInTheDocument();
    expect(within(col).queryByText('暂无')).not.toBeInTheDocument();
    const c1 = screen.getByTestId('signal-card-1');
    expect(within(c1).getByText('Signal 1')).toBeInTheDocument();
    expect(within(c1).getByText('sentry')).toBeInTheDocument();
    expect(within(c1).getByText('严重')).toBeInTheDocument();
    expect(within(c1).getByText(/^触发 3 次/)).toBeInTheDocument();
    expect(within(c1).getByRole('link', { name: '来源' })).toHaveAttribute('href', 'https://sentry.local/issues/1/');
    const c2 = screen.getByTestId('signal-card-2');
    expect(within(c2).getByText('已恢复')).toBeInTheDocument();
    expect(within(c2).getByText('提示')).toBeInTheDocument();
  });

  it('opens the evidence drawer', async () => {
    const user = userEvent.setup();
    renderBoard([signal(1)]);
    await user.click(within(screen.getByTestId('signal-card-1')).getByRole('button', { name: 'Signal 1' }));
    const drawer = await screen.findByTestId('signal-detail');
    expect(within(drawer).getByText('boom')).toBeInTheDocument();
    expect(within(drawer).getByText(/a\.py:1/)).toBeInTheDocument();
    expect(within(drawer).getByText(/首次 2026-11-14 08:00:00/)).toBeInTheDocument();
  });

  it('ignores for a period and refreshes the board', async () => {
    const user = userEvent.setup();
    const { onChanged } = renderBoard([signal(1, { projectId: 42 })]);
    await user.click(within(screen.getByTestId('signal-card-1')).getByRole('button', { name: '忽略' }));
    await user.click(await screen.findByRole('menuitem', { name: '忽略 7 天' }));
    await waitFor(() => expect(ignored).toEqual([{ pid: '42', id: '1', days: 7 }]));
    expect(onChanged).toHaveBeenCalled();
    await user.click(within(screen.getByTestId('signal-card-1')).getByRole('button', { name: '忽略' }));
    await user.click(await screen.findByRole('menuitem', { name: '永久忽略' }));
    await waitFor(() => expect(ignored).toHaveLength(2));
    expect(ignored[1]).toEqual({ pid: '42', id: '1', days: null });
  });
});
