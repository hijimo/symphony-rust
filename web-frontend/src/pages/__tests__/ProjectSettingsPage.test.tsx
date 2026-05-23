import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import ProjectSettingsPage from '../projects/ProjectSettingsPage';
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

function renderProjectSettings(projectId = '1') {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={[`/projects/${projectId}/settings`]}>
        <Routes>
          <Route path="/projects/:id/settings" element={<ProjectSettingsPage />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('ProjectSettingsPage', () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    });
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('renders tabs (基本信息, 工作流, 服务控制, 危险操作)', async () => {
    renderProjectSettings();

    await waitFor(() => {
      expect(screen.getByText(/项目设置/)).toBeInTheDocument();
    });

    expect(screen.getByRole('tab', { name: '基本信息' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: '工作流' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: '服务控制' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: '危险操作' })).toBeInTheDocument();
  });

  it('loads project data on mount', async () => {
    renderProjectSettings();

    await waitFor(() => {
      expect(screen.getByText('项目设置 - My GitLab Project')).toBeInTheDocument();
    });

    // Basic info fields should be populated
    expect(screen.getByLabelText('项目名称')).toHaveValue('My GitLab Project');
    expect(screen.getByLabelText('默认分支')).toHaveValue('main');
  });

  it('saves basic info changes', async () => {
    const user = userEvent.setup();
    renderProjectSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('项目名称')).toHaveValue('My GitLab Project');
    });

    // Change the name
    const nameInput = screen.getByLabelText('项目名称');
    await user.clear(nameInput);
    await user.type(nameInput, 'Updated Project Name');

    // Save button should be enabled
    const saveBtn = screen.getByRole('button', { name: '保存' });
    expect(saveBtn).not.toBeDisabled();
    await user.click(saveBtn);

    await waitFor(() => {
      expect(screen.getByText('项目信息已更新')).toBeInTheDocument();
    });
  });

  it('delete button shows confirmation dialog', async () => {
    const user = userEvent.setup();
    renderProjectSettings();

    await waitFor(() => {
      expect(screen.getByText('项目设置 - My GitLab Project')).toBeInTheDocument();
    });

    // Switch to danger zone tab
    await user.click(screen.getByRole('tab', { name: '危险操作' }));

    // Click delete button
    await user.click(screen.getByRole('button', { name: '删除此项目' }));

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: '确认删除' })).toBeInTheDocument();
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

    renderProjectSettings('999');

    await waitFor(() => {
      expect(screen.getByText('项目不存在或无权访问')).toBeInTheDocument();
    });
  });

  it('workflow tab loads workflow data', async () => {
    const user = userEvent.setup();
    renderProjectSettings();

    await waitFor(() => {
      expect(screen.getByText('项目设置 - My GitLab Project')).toBeInTheDocument();
    });

    // Switch to workflow tab
    await user.click(screen.getByRole('tab', { name: '工作流' }));

    await waitFor(() => {
      expect(screen.getByLabelText('默认模板')).toBeInTheDocument();
      expect(screen.getByLabelText('自定义')).toBeInTheDocument();
    });
  });
});
