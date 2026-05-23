import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import Login from '../Login';
import { useAuthStore } from '../../store/auth';
import { server } from '../../test/mocks/server';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

function renderLogin() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/login']}>
        <Login />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('Login', () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    useAuthStore.setState({
      token: null,
      user: null,
      isAuthenticated: false,
    });
  });

  it('renders login form with username, password, and submit button', () => {
    renderLogin();
    expect(screen.getByLabelText('用户名')).toBeInTheDocument();
    expect(screen.getByLabelText('密码')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '登录' })).toBeInTheDocument();
  });

  it('shows validation errors on empty submit', async () => {
    const user = userEvent.setup();
    renderLogin();

    await user.click(screen.getByRole('button', { name: '登录' }));

    expect(screen.getByText('请输入用户名')).toBeInTheDocument();
    expect(screen.getByText('请输入密码')).toBeInTheDocument();
  });

  it('shows username error when only password is filled', async () => {
    const user = userEvent.setup();
    renderLogin();

    await user.type(screen.getByLabelText('密码'), 'somepass');
    await user.click(screen.getByRole('button', { name: '登录' }));

    expect(screen.getByText('请输入用户名')).toBeInTheDocument();
    expect(screen.queryByText('请输入密码')).not.toBeInTheDocument();
  });

  it('calls API and navigates on successful login', async () => {
    const user = userEvent.setup();
    renderLogin();

    await user.type(screen.getByLabelText('用户名'), 'admin');
    await user.type(screen.getByLabelText('密码'), 'Admin@123456');
    await user.click(screen.getByRole('button', { name: '登录' }));

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalled();
    });

    const state = useAuthStore.getState();
    expect(state.isAuthenticated).toBe(true);
    expect(state.token).toBe('mock-jwt-token');
  });

  it('shows error message on login failure', async () => {
    const user = userEvent.setup();
    renderLogin();

    await user.type(screen.getByLabelText('用户名'), 'admin');
    await user.type(screen.getByLabelText('密码'), 'wrongpass');
    await user.click(screen.getByRole('button', { name: '登录' }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('用户名或密码错误');
    });
  });

  it('disables button and shows loading during submission', async () => {
    const user = userEvent.setup();
    server.use(
      http.post('*/api/auth/login', async () => {
        await new Promise((r) => setTimeout(r, 100));
        return HttpResponse.json({
          success: true,
          retCode: 'SUCCESS',
          retMsg: 'ok',
          data: {
            token: 'tok',
            expiresAt: '2099-01-01T00:00:00Z',
            user: { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' },
          },
        });
      }),
    );
    renderLogin();

    await user.type(screen.getByLabelText('用户名'), 'admin');
    await user.type(screen.getByLabelText('密码'), 'Admin@123456');
    await user.click(screen.getByRole('button', { name: '登录' }));

    expect(screen.getByRole('button', { name: '' })).toBeDisabled();

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalled();
    });
  });

  it('shows network error message', async () => {
    const user = userEvent.setup();
    server.use(
      http.post('*/api/auth/login', () => {
        return HttpResponse.error();
      }),
    );
    renderLogin();

    await user.type(screen.getByLabelText('用户名'), 'admin');
    await user.type(screen.getByLabelText('密码'), 'Admin@123456');
    await user.click(screen.getByRole('button', { name: '登录' }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument();
    });
  });
});
