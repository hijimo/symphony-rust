import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import Settings from '../Settings';
import { useAuthStore } from '../../store/auth';
import { server } from '../../test/mocks/server';

function renderSettings() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/settings']}>
        <Settings />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('Settings', () => {
  beforeEach(() => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    });
    localStorage.setItem('token', 'mock-token');
  });

  it('renders profile section after loading', async () => {
    renderSettings();

    await waitFor(() => {
      expect(screen.getByText('个人设置')).toBeInTheDocument();
    });
    expect(screen.getByText('个人信息')).toBeInTheDocument();
  });

  it('loads and displays display name', async () => {
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('显示名')).toHaveValue('Administrator');
    });
  });

  it('enables save button when display name changes', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('显示名')).toHaveValue('Administrator');
    });

    const saveButtons = screen.getAllByRole('button', { name: '保存' });
    expect(saveButtons[0]).toBeDisabled();

    const input = screen.getByLabelText('显示名');
    await user.clear(input);
    await user.type(input, 'New Name');

    expect(saveButtons[0]).toBeEnabled();
  });

  it('saves profile successfully', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('显示名')).toHaveValue('Administrator');
    });

    const input = screen.getByLabelText('显示名');
    await user.clear(input);
    await user.type(input, 'New Name');

    const saveButtons = screen.getAllByRole('button', { name: '保存' });
    await user.click(saveButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('个人信息已更新')).toBeInTheDocument();
    });
  });

  it('validates password fields', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('当前密码')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('当前密码'), 'old');

    const changePwdBtn = screen.getByRole('button', { name: '修改密码' });
    await user.click(changePwdBtn);

    await waitFor(() => {
      expect(screen.getByText('新密码至少6个字符')).toBeInTheDocument();
    });
  });

  it('shows password mismatch error', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('当前密码')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('当前密码'), 'oldpass');
    await user.type(screen.getByLabelText('新密码'), 'NewPass123');
    await user.type(screen.getByLabelText('确认新密码'), 'Different1');

    await user.click(screen.getByRole('button', { name: '修改密码' }));

    await waitFor(() => {
      expect(screen.getByText('两次输入的密码不一致')).toBeInTheDocument();
    });
  });

  it('changes password successfully', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('当前密码')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('当前密码'), 'OldPass123');
    await user.type(screen.getByLabelText('新密码'), 'NewPass123');
    await user.type(screen.getByLabelText('确认新密码'), 'NewPass123');

    await user.click(screen.getByRole('button', { name: '修改密码' }));

    await waitFor(() => {
      expect(screen.getByText('密码修改成功')).toBeInTheDocument();
    });
  });

  it('shows old password error from API', async () => {
    const user = userEvent.setup();
    server.use(
      http.put('*/api/auth/password', () => {
        return HttpResponse.json(
          { success: false, retCode: 'AUTH_003', retMsg: '密码不正确', data: null },
          { status: 400 },
        );
      }),
    );
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('当前密码')).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText('当前密码'), 'wrong');
    await user.type(screen.getByLabelText('新密码'), 'NewPass123');
    await user.type(screen.getByLabelText('确认新密码'), 'NewPass123');

    await user.click(screen.getByRole('button', { name: '修改密码' }));

    await waitFor(() => {
      expect(screen.getByText('当前密码不正确')).toBeInTheDocument();
    });
  });

  it('renders token configuration section', async () => {
    renderSettings();

    await waitFor(() => {
      expect(screen.getByText('Token 配置')).toBeInTheDocument();
    });
    expect(screen.getByText('GitLab Token')).toBeInTheDocument();
    expect(screen.getByText('GitHub Token')).toBeInTheDocument();
  });

  it('shows token configured status', async () => {
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('GitLab Token 状态：已配置')).toBeInTheDocument();
    });
    expect(screen.getByLabelText('GitHub Token 状态：未配置')).toBeInTheDocument();
  });

  it('saves token configuration', async () => {
    const user = userEvent.setup();
    renderSettings();

    await waitFor(() => {
      expect(screen.getByText('Token 配置')).toBeInTheDocument();
    });

    const gitlabHostInput = screen.getByLabelText('GitLab Host');
    await user.clear(gitlabHostInput);
    await user.type(gitlabHostInput, 'https://new-gitlab.com');

    await user.click(screen.getByRole('button', { name: '保存 Token 配置' }));

    await waitFor(() => {
      expect(screen.getByText('Token 配置已保存')).toBeInTheDocument();
    });
  });
});
