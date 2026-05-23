import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import ProjectCard from '../ProjectCard';
import type { Project } from '../../../types';

const mockNavigate = vi.fn();
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual('react-router-dom');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

const baseProject: Project = {
  id: 1,
  name: 'Test Project',
  description: 'A test project description',
  git_url: 'https://gitlab.com/group/test-project.git',
  platform: 'gitlab',
  platform_host: 'gitlab.com',
  namespace: 'group',
  repo_name: 'test-project',
  default_branch: 'main',
  workflow_template: 'default',
  service_status: 'running',
  service_pid: 12345,
  max_concurrent_agents: 2,
  auto_restart: true,
  member_count: 3,
  my_role: 'owner',
  created_by: 1,
  created_at: '2024-01-10T00:00:00Z',
  updated_at: '2024-01-10T00:00:00Z',
  hooks_after_create: null,
  hooks_before_remove: null,
  codex_command: null,
  codex_approval_policy: null,
  codex_sandbox: null,
};

function renderCard(project: Partial<Project> = {}, overrides = {}) {
  const props = {
    project: { ...baseProject, ...project },
    onStart: vi.fn().mockResolvedValue(undefined),
    onStop: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  return {
    ...render(
      <ThemeProvider theme={theme}>
        <MemoryRouter>
          <ProjectCard {...props} />
        </MemoryRouter>
      </ThemeProvider>,
    ),
    props,
  };
}

describe('ProjectCard', () => {
  beforeEach(() => {
    mockNavigate.mockClear();
  });

  it('renders project name', () => {
    renderCard();
    expect(screen.getByText('Test Project')).toBeInTheDocument();
  });

  it('renders project description', () => {
    renderCard();
    expect(screen.getByText('A test project description')).toBeInTheDocument();
  });

  it('renders namespace/repo', () => {
    renderCard();
    expect(screen.getByText('group/test-project')).toBeInTheDocument();
  });

  it('shows correct service status badge', () => {
    renderCard({ service_status: 'running' });
    expect(screen.getByText('运行中')).toBeInTheDocument();
  });

  it('shows GitLab platform badge', () => {
    renderCard({ platform: 'gitlab' });
    expect(screen.getByText('GitLab')).toBeInTheDocument();
  });

  it('shows GitHub platform badge', () => {
    renderCard({ platform: 'github' });
    expect(screen.getByText('GitHub')).toBeInTheDocument();
  });

  it('shows member count', () => {
    renderCard({ member_count: 3 });
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('shows stop button when running and user is owner', () => {
    renderCard({ service_status: 'running', my_role: 'owner' });
    expect(screen.getByLabelText('停止服务')).toBeInTheDocument();
  });

  it('shows start button when stopped and user is owner', () => {
    renderCard({ service_status: 'stopped', my_role: 'owner' });
    expect(screen.getByLabelText('启动服务')).toBeInTheDocument();
  });

  it('does not show control buttons when user is member', () => {
    renderCard({ service_status: 'running', my_role: 'member' });
    expect(screen.queryByLabelText('停止服务')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('启动服务')).not.toBeInTheDocument();
  });

  it('triggers onStart when start button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderCard({ service_status: 'stopped', my_role: 'owner' });

    await user.click(screen.getByLabelText('启动服务'));

    await waitFor(() => {
      expect(props.onStart).toHaveBeenCalledWith(1);
    });
  });

  it('triggers onStop when stop button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderCard({ service_status: 'running', my_role: 'owner' });

    await user.click(screen.getByLabelText('停止服务'));

    await waitFor(() => {
      expect(props.onStop).toHaveBeenCalledWith(1);
    });
  });

  it('navigates to project detail on card click', async () => {
    const user = userEvent.setup();
    renderCard();

    await user.click(screen.getByRole('article', { name: '项目 Test Project' }));

    expect(mockNavigate).toHaveBeenCalledWith('/projects/1');
  });

  it('does not show description when it is null', () => {
    renderCard({ description: null });
    expect(screen.queryByText('A test project description')).not.toBeInTheDocument();
  });
});
