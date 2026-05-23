import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import AdminUsers from '../AdminUsers';
import { useAuthStore } from '../../store/auth';
import { server } from '../../test/mocks/server';

function renderAdminUsers() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/admin/users']}>
        <AdminUsers />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('AdminUsers', () => {
  beforeEach(() => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    });
    localStorage.setItem('token', 'mock-token');
  });

  it('renders user list after loading', async () => {
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });
    expect(screen.getByText('John Doe')).toBeInTheDocument();
    expect(screen.getByText('用户管理')).toBeInTheDocument();
  });

  it('shows loading skeleton initially', () => {
    renderAdminUsers();
    expect(screen.getByText('用户管理')).toBeInTheDocument();
  });

  it('filters users by search input', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    const searchInput = screen.getByLabelText('搜索用户');
    await user.type(searchInput, 'john');

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });
  });

  it('opens add user dialog', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '添加用户' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(within(screen.getByRole('dialog')).getByText('添加用户')).toBeInTheDocument();
  });

  it('creates user successfully and closes dialog', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '添加用户' }));

    const dialog = screen.getByRole('dialog');
    const usernameInput = within(dialog).getByLabelText('用户名 *');
    const passwordInput = within(dialog).getByLabelText('密码 *');

    await user.type(usernameInput, 'newuser');
    await user.type(passwordInput, 'Pass123456');
    await user.click(within(dialog).getByRole('button', { name: '确认添加' }));

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  it('shows validation error for short username', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '添加用户' }));
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: '确认添加' }));

    expect(screen.getByText('用户名需要3-32个字符')).toBeInTheDocument();
  });

  it('opens delete confirmation dialog', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    const deleteBtn = screen.getByLabelText('删除用户John Doe');
    await user.click(deleteBtn);

    await waitFor(() => {
      expect(screen.getByText('删除用户')).toBeInTheDocument();
    });
    expect(screen.getByText(/确定要删除用户/)).toBeInTheDocument();
  });

  it('deletes user on confirm', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    const deleteBtn = screen.getByLabelText('删除用户John Doe');
    await user.click(deleteBtn);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '确认删除' })).toBeInTheDocument();
    });
    await user.click(screen.getByRole('button', { name: '确认删除' }));

    await waitFor(() => {
      expect(screen.getByText('用户已删除')).toBeInTheDocument();
    });
  });

  it('opens reset password dialog', async () => {
    const user = userEvent.setup();
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    const resetBtn = screen.getByLabelText('重置用户John Doe的密码');
    await user.click(resetBtn);

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });
    expect(within(screen.getByRole('dialog')).getByText('重置密码')).toBeInTheDocument();
  });

  it('shows error when API fails', async () => {
    server.use(
      http.get('*/api/admin/users', () => {
        return HttpResponse.json(
          { success: false, retCode: 'ERR', retMsg: '获取用户列表失败', data: null },
          { status: 500 },
        );
      }),
    );
    renderAdminUsers();

    await waitFor(() => {
      expect(screen.getByText('获取用户列表失败')).toBeInTheDocument();
    });
  });
});
