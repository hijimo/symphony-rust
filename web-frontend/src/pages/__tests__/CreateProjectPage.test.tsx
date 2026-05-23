import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import CreateProjectPage from '../projects/CreateProjectPage';
import { useAuthStore } from '../../store/auth';
import { useProjectStore } from '../../store/projectStore';
import { server } from '../../test/mocks/server';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

function renderCreateProject() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects/new']}>
        <CreateProjectPage />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('CreateProjectPage', () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
      isAuthenticated: true,
    });
    useProjectStore.setState({
      projects: [],
      currentProject: null,
      loading: false,
      pagination: { pageNo: 1, pageSize: 20, totalCount: 0, pages: 0 },
    });
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('renders form fields', () => {
    renderCreateProject();

    expect(screen.getByRole('heading', { name: '创建项目' })).toBeInTheDocument();
    expect(screen.getByLabelText(/Git URL/)).toBeInTheDocument();
    expect(screen.getByLabelText('项目名称')).toBeInTheDocument();
    expect(screen.getByLabelText('项目描述')).toBeInTheDocument();
    expect(screen.getByLabelText('默认分支')).toBeInTheDocument();
  });

  it('validates git URL input - shows error for empty URL', async () => {
    const user = userEvent.setup();
    renderCreateProject();

    // Type something then clear to enable the button, or just click submit
    // The button is disabled when gitUrl is empty, so we need to type something invalid
    const gitInput = screen.getByLabelText(/Git URL/);
    await user.type(gitInput, 'invalid-url');

    const submitBtn = screen.getByRole('button', { name: '创建项目' });
    await user.click(submitBtn);

    await waitFor(() => {
      expect(screen.getByText('无效的 Git URL 格式，请输入 HTTPS 或 SSH 格式的地址')).toBeInTheDocument();
    });
  });

  it('shows parsed URL preview for valid git URL', async () => {
    const user = userEvent.setup();
    renderCreateProject();

    const gitInput = screen.getByLabelText(/Git URL/);
    await user.type(gitInput, 'https://github.com/org/my-repo.git');

    await waitFor(() => {
      expect(screen.getByText('解析结果')).toBeInTheDocument();
      expect(screen.getByText('GitHub')).toBeInTheDocument();
    });
  });

  it('submits form and navigates on success', async () => {
    const user = userEvent.setup();
    renderCreateProject();

    const gitInput = screen.getByLabelText(/Git URL/);
    await user.type(gitInput, 'https://github.com/org/my-repo.git');

    const submitBtn = screen.getByRole('button', { name: '创建项目' });
    await user.click(submitBtn);

    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalledWith('/projects/3', { replace: true });
    });
  });

  it('shows error on API failure', async () => {
    server.use(
      http.post('*/api/projects', () => {
        return HttpResponse.json(
          { success: false, retCode: 'ERR', retMsg: '创建项目失败', data: null },
          { status: 500 },
        );
      }),
    );

    const user = userEvent.setup();
    renderCreateProject();

    const gitInput = screen.getByLabelText(/Git URL/);
    await user.type(gitInput, 'https://github.com/org/my-repo.git');

    const submitBtn = screen.getByRole('button', { name: '创建项目' });
    await user.click(submitBtn);

    await waitFor(() => {
      expect(screen.getByText('创建项目失败')).toBeInTheDocument();
    });
  });

  it('cancel button navigates back to projects list', async () => {
    const user = userEvent.setup();
    renderCreateProject();

    await user.click(screen.getByRole('button', { name: '取消' }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects');
  });

  it('back button navigates to projects list', async () => {
    const user = userEvent.setup();
    renderCreateProject();

    await user.click(screen.getByRole('button', { name: '返回' }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects');
  });

  it('submit button is disabled when git URL is empty', () => {
    renderCreateProject();

    const submitBtn = screen.getByRole('button', { name: '创建项目' });
    expect(submitBtn).toBeDisabled();
  });
});
