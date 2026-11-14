import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import { useAuthStore } from '../../../store/auth';
import GateApproversEditor from '../GateApproversEditor';

const PID = 4;
const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });
const members = [
  { user_id: 1, username: 'alice', display_name: 'Alice', role: 'owner', synced_from: null, created_at: '' },
  { user_id: 2, username: 'bob', display_name: null, role: 'member', synced_from: null, created_at: '' },
];

function renderEditor() {
  render(
    <ThemeProvider theme={theme}>
      <GateApproversEditor projectId={PID} />
    </ThemeProvider>,
  );
}

describe('GateApproversEditor', () => {
  let puts: unknown[];
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    puts = [];
    server.use(
      http.get(`*/api/projects/${PID}/members`, () => ok(members)),
      http.get(`*/api/projects/${PID}/gate-approvers`, () =>
        ok({ G1: [], G2: [{ userId: 2, username: 'bob', displayName: null }], G3: [] }),
      ),
      http.put(`*/api/projects/${PID}/gate-approvers`, async ({ request }) => {
        const body = (await request.json()) as Record<string, number[]>;
        puts.push(body);
        const named = (ids: number[]) => ids.map((id) => ({ userId: id, username: id === 1 ? 'alice' : 'bob', displayName: null }));
        return ok({ G1: named(body.G1), G2: named(body.G2), G3: named(body.G3) });
      }),
    );
  });

  it('owners toggle approvers per gate and save', async () => {
    useAuthStore.setState({ user: { id: 1, username: 'alice', displayName: 'Alice', role: 'user' } });
    const user = userEvent.setup();
    renderEditor();
    const bobG2 = await screen.findByRole('checkbox', { name: 'bob 可审批 G2' });
    expect(bobG2).toBeChecked();
    expect(screen.getByText('G1 · intent：所有成员可审批')).toBeInTheDocument();
    const save = screen.getByRole('button', { name: '保存审批人' });
    expect(save).toBeDisabled();
    await user.click(screen.getByRole('checkbox', { name: 'Alice 可审批 G1' }));
    expect(screen.getByText('G1 · intent：仅 Alice')).toBeInTheDocument();
    await user.click(save);
    await waitFor(() => expect(puts).toEqual([{ G1: [1], G2: [2], G3: [] }]));
    expect(await screen.findByText('已保存')).toBeInTheDocument();
  });

  it('members see the lists read-only', async () => {
    useAuthStore.setState({ user: { id: 2, username: 'bob', displayName: null, role: 'user' } });
    renderEditor();
    expect(await screen.findByRole('checkbox', { name: 'bob 可审批 G2' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: '保存审批人' })).not.toBeInTheDocument();
    expect(screen.getByText('只有项目 owner 可以修改审批人')).toBeInTheDocument();
  });
});
