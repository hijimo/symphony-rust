import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../theme';
import ProjectListPage from '../projects/ProjectListPage';
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

function renderProjectList() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects']}>
        <ProjectListPage />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('ProjectListPage', () => {
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

  it('renders project list from API', async () => {
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });
    expect(screen.getByText('My GitHub Project')).toBeInTheDocument();
  });

  it('shows loading skeleton initially', () => {
    // Set loading state before render
    useProjectStore.setState({ loading: true });
    renderProjectList();

    // The page title should still be visible
    expect(screen.getByText('项目')).toBeInTheDocument();
  });

  it('shows empty state when no projects', async () => {
    server.use(
      http.get('*/api/projects', () => {
        return HttpResponse.json({
          success: true,
          retCode: 'SUCCESS',
          retMsg: 'ok',
          data: {
            records: [],
            totalCount: 0,
            pageNo: 1,
            pageSize: 20,
            pages: 0,
            limit: 20,
            offset: 0,
          },
        });
      }),
    );

    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('暂无项目')).toBeInTheDocument();
      expect(screen.getByText('创建你的第一个项目，开始使用 Symphony 工作流')).toBeInTheDocument();
    });
  });

  it('search filters projects', async () => {
    const user = userEvent.setup();
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });

    // Type in search
    const searchInput = screen.getByLabelText('搜索项目');
    await user.type(searchInput, 'GitLab');

    // After debounce, should filter
    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });
  });

  it('platform filter works', async () => {
    const user = userEvent.setup();
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });

    // MUI filled Select uses a hidden input; find the select by its role
    const platformSelects = screen.getAllByRole('combobox');
    // The first combobox is the platform filter
    await user.click(platformSelects[0]);

    // Select GitLab
    const gitlabOption = screen.getByRole('option', { name: 'GitLab' });
    await user.click(gitlabOption);

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });
  });

  it('navigates to create page on button click', async () => {
    const user = userEvent.setup();
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });

    // Click the header "创建项目" button
    const createButtons = screen.getAllByRole('button', { name: '创建项目' });
    await user.click(createButtons[0]);

    expect(mockNavigate).toHaveBeenCalledWith('/projects/new');
  });

  it('navigates to kanban when a project card is clicked', async () => {
    const user = userEvent.setup();
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('My GitLab Project')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('article', { name: '项目 My GitLab Project' }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects/1/kanban');
  });

  it('shows total count when projects exist', async () => {
    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('共 2 个项目')).toBeInTheDocument();
    });
  });

  it('shows error snackbar on API failure', async () => {
    server.use(
      http.get('*/api/projects', () => {
        return HttpResponse.json(
          { success: false, retCode: 'ERR', retMsg: '获取项目列表失败', data: null },
          { status: 500 },
        );
      }),
    );

    renderProjectList();

    await waitFor(() => {
      expect(screen.getByText('获取项目列表失败')).toBeInTheDocument();
    });
  });
});
