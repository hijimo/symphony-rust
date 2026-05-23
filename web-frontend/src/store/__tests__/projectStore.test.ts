import { describe, it, expect, beforeEach } from 'vitest';
import { useProjectStore } from '../projectStore';

describe('useProjectStore', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    useProjectStore.setState({
      projects: [],
      currentProject: null,
      loading: false,
      pagination: { pageNo: 1, pageSize: 20, totalCount: 0, pages: 0 },
    });
  });

  it('has correct initial state', () => {
    const state = useProjectStore.getState();
    expect(state.projects).toEqual([]);
    expect(state.loading).toBe(false);
    expect(state.pagination.totalCount).toBe(0);
  });

  it('fetchProjects updates state with project list', async () => {
    await useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });

    const state = useProjectStore.getState();
    expect(state.projects.length).toBe(2);
    expect(state.projects[0].name).toBe('My GitLab Project');
    expect(state.projects[1].name).toBe('My GitHub Project');
    expect(state.pagination.totalCount).toBe(2);
    expect(state.loading).toBe(false);
  });

  it('fetchProjects sets loading to true during fetch', async () => {
    const promise = useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });
    // loading should be true while fetching
    expect(useProjectStore.getState().loading).toBe(true);
    await promise;
    expect(useProjectStore.getState().loading).toBe(false);
  });

  it('createProject returns the created project', async () => {
    const project = await useProjectStore.getState().createProject({
      git_url: 'https://github.com/org/new-repo.git',
      name: 'New Repo',
    });

    expect(project.id).toBe(3);
    expect(project.name).toBe('New Repo');
  });

  it('deleteProject removes project from list', async () => {
    // First populate the store
    await useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });
    expect(useProjectStore.getState().projects.length).toBe(2);

    // Delete project
    await useProjectStore.getState().deleteProject(1);

    const state = useProjectStore.getState();
    expect(state.projects.length).toBe(1);
    expect(state.projects[0].id).toBe(2);
  });

  it('startService updates project status to running', async () => {
    // First populate the store
    await useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });

    // Start service for project 2 (which is stopped)
    await useProjectStore.getState().startService(2);

    const state = useProjectStore.getState();
    const project = state.projects.find((p) => p.id === 2);
    expect(project?.service_status).toBe('running');
  });

  it('stopService updates project status to stopped', async () => {
    // First populate the store
    await useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });

    // Stop service for project 1 (which is running)
    await useProjectStore.getState().stopService(1);

    const state = useProjectStore.getState();
    const project = state.projects.find((p) => p.id === 1);
    expect(project?.service_status).toBe('stopped');
  });

  it('restartService updates project status', async () => {
    // First populate the store
    await useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 });

    await useProjectStore.getState().restartService(1);

    const state = useProjectStore.getState();
    const project = state.projects.find((p) => p.id === 1);
    expect(project?.service_status).toBe('running');
  });

  it('handles API errors gracefully in fetchProjects', async () => {
    localStorage.removeItem('token');
    localStorage.removeItem('expiresAt');

    await expect(
      useProjectStore.getState().fetchProjects({ pageNo: 1, pageSize: 20 }),
    ).rejects.toThrow();

    const state = useProjectStore.getState();
    expect(state.loading).toBe(false);
  });

  it('updateProjectInList updates specific project fields', () => {
    useProjectStore.setState({
      projects: [
        {
          id: 1,
          name: 'Test',
          description: null,
          git_url: 'https://gitlab.com/g/r.git',
          platform: 'gitlab',
          platform_host: 'gitlab.com',
          namespace: 'g',
          repo_name: 'r',
          default_branch: 'main',
          workflow_template: 'default',
          service_status: 'stopped',
          service_pid: null,
          max_concurrent_agents: 2,
          auto_restart: true,
          member_count: 1,
          my_role: 'owner',
          created_by: 1,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          hooks_after_create: null,
          hooks_before_remove: null,
          codex_command: null,
          codex_approval_policy: null,
          codex_sandbox: null,
        },
      ],
    });

    useProjectStore.getState().updateProjectInList(1, { service_status: 'running' });

    const state = useProjectStore.getState();
    expect(state.projects[0].service_status).toBe('running');
    expect(state.projects[0].name).toBe('Test');
  });

  it('setCurrentProject updates currentProject', () => {
    const project = {
      id: 1,
      name: 'Test',
      description: null,
      git_url: 'https://gitlab.com/g/r.git',
      platform: 'gitlab' as const,
      platform_host: 'gitlab.com',
      namespace: 'g',
      repo_name: 'r',
      default_branch: 'main',
      workflow_template: 'default' as const,
      service_status: 'stopped' as const,
      service_pid: null,
      max_concurrent_agents: 2,
      auto_restart: true,
      member_count: 1,
      my_role: 'owner' as const,
      created_by: 1,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      hooks_after_create: null,
      hooks_before_remove: null,
      codex_command: null,
      codex_approval_policy: null,
      codex_sandbox: null,
    };

    useProjectStore.getState().setCurrentProject(project);
    expect(useProjectStore.getState().currentProject).toEqual(project);

    useProjectStore.getState().setCurrentProject(null);
    expect(useProjectStore.getState().currentProject).toBeNull();
  });
});
