import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { describe, it, expect, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import ProjectMembersPage from '../projects/ProjectMembersPage';
import { useAuthStore } from '../../store/auth';
import { server } from '../../test/mocks/server';

function renderProjectMembers(projectId = '1') {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={[`/projects/${projectId}/members`]}>
        <Routes>
          <Route path="/projects/:id/members" element={<ProjectMembersPage />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('ProjectMembersPage', () => {
  beforeEach(() => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    });
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('renders member table with members', async () => {
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('项目成员 - My GitLab Project')).toBeInTheDocument();
    });

    expect(screen.getByText('admin')).toBeInTheDocument();
    expect(screen.getByText('john')).toBeInTheDocument();
    expect(screen.getByText('John Doe')).toBeInTheDocument();
  });

  it('shows add member button for admin users', async () => {
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('项目成员 - My GitLab Project')).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: '添加成员' })).toBeInTheDocument();
  });

  it('add member dialog opens on button click', async () => {
    const user = userEvent.setup();
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('项目成员 - My GitLab Project')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '添加成员' }));

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });
  });

  it('role change dropdown works for non-self members', async () => {
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    // John's role should have a select dropdown since current user is admin
    const roleSelect = screen.getByLabelText('修改 John Doe 的角色');
    expect(roleSelect).toBeInTheDocument();
  });

  it('remove member shows confirmation dialog', async () => {
    const user = userEvent.setup();
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    // Click remove button for John
    const removeBtn = screen.getByLabelText('移除成员 John Doe');
    await user.click(removeBtn);

    await waitFor(() => {
      expect(screen.getByText('移除成员')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: '确认移除' })).toBeInTheDocument();
    });
  });

  it('remove member confirms and removes', async () => {
    const user = userEvent.setup();
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('john')).toBeInTheDocument();
    });

    const removeBtn = screen.getByLabelText('移除成员 John Doe');
    await user.click(removeBtn);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '确认移除' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '确认移除' }));

    await waitFor(() => {
      expect(screen.getByText('成员已移除')).toBeInTheDocument();
    });
  });

  it('sync button shows results', async () => {
    const user = userEvent.setup();
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('项目成员 - My GitLab Project')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '从平台同步' }));

    await waitFor(() => {
      expect(screen.getByText(/同步完成/)).toBeInTheDocument();
    });
  });

  it('shows error when project not found', async () => {
    server.use(
      http.get('*/api/projects/:id', () => {
        return HttpResponse.json(
          { success: false, retCode: 'NOT_FOUND', retMsg: '项目不存在', data: null },
          { status: 404 },
        );
      }),
    );

    renderProjectMembers('999');

    await waitFor(() => {
      expect(screen.getByText('项目不存在或无权访问')).toBeInTheDocument();
    });
  });

  it('shows sync result details after sync', async () => {
    const user = userEvent.setup();
    renderProjectMembers();

    await waitFor(() => {
      expect(screen.getByText('项目成员 - My GitLab Project')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: '从平台同步' }));

    await waitFor(() => {
      expect(screen.getByText('同步结果')).toBeInTheDocument();
      expect(screen.getByText('新增 2 人')).toBeInTheDocument();
      expect(screen.getByText('跳过 1 人')).toBeInTheDocument();
    });
  });
});
