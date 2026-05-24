import { http, HttpResponse } from 'msw';

const BASE_URL = '*/api';

const mockAdmin = {
  id: 1,
  username: 'admin',
  displayName: 'Administrator',
  role: 'admin',
  createdAt: '2024-01-01T00:00:00Z',
  updatedAt: '2024-01-01T00:00:00Z',
};

const mockUser = {
  id: 2,
  username: 'john',
  displayName: 'John Doe',
  role: 'user',
  createdAt: '2024-01-02T00:00:00Z',
  updatedAt: '2024-01-02T00:00:00Z',
};

const mockUsersList = [mockAdmin, mockUser];

const mockProjects = [
  {
    id: 1,
    name: 'My GitLab Project',
    description: 'A test project on GitLab',
    git_url: 'https://gitlab.com/group/my-project.git',
    platform: 'gitlab' as const,
    platform_host: 'gitlab.com',
    namespace: 'group',
    repo_name: 'my-project',
    default_branch: 'main',
    workflow_template: 'default' as const,
    service_status: 'running' as const,
    service_pid: 12345,
    max_concurrent_agents: 2,
    auto_restart: true,
    member_count: 3,
    my_role: 'owner' as const,
    created_by: 1,
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-10T00:00:00Z',
  },
  {
    id: 2,
    name: 'My GitHub Project',
    description: 'A test project on GitHub',
    git_url: 'https://github.com/org/another-project.git',
    platform: 'github' as const,
    platform_host: 'github.com',
    namespace: 'org',
    repo_name: 'another-project',
    default_branch: 'main',
    workflow_template: 'custom' as const,
    service_status: 'stopped' as const,
    service_pid: null,
    max_concurrent_agents: 4,
    auto_restart: false,
    member_count: 2,
    my_role: 'member' as const,
    created_by: 2,
    created_at: '2024-02-01T00:00:00Z',
    updated_at: '2024-02-01T00:00:00Z',
  },
];

const mockMembers = [
  {
    user_id: 1,
    username: 'admin',
    display_name: 'Administrator',
    role: 'owner' as const,
    synced_from: null,
    created_at: '2024-01-10T00:00:00Z',
  },
  {
    user_id: 2,
    username: 'john',
    display_name: 'John Doe',
    role: 'member' as const,
    synced_from: 'gitlab' as const,
    created_at: '2024-01-15T00:00:00Z',
  },
];

type MockProxyMode = 'disabled' | 'inherit_env' | 'manual';

interface MockProxySecret {
  configured: boolean;
  displayValue: string;
  updatedAt: string | null;
}

interface MockNetworkProxy {
  mode: MockProxyMode;
  version: string;
  source: string;
  httpProxy: MockProxySecret;
  httpsProxy: MockProxySecret;
  allProxy: MockProxySecret;
  noProxy: string;
  autoBypassLocal: boolean;
  needsRestartProjectCount: number;
  updatedAt: string | null;
  warnings: Array<{
    code: string;
    severity: 'info' | 'warning' | 'error';
    blocking: boolean;
    message: string;
  }>;
}

let mockNetworkProxy: MockNetworkProxy = {
  mode: 'disabled',
  version: 'proxy-v1',
  source: 'database',
  httpProxy: {
    configured: true,
    displayValue: 'http://us***@proxy.internal:8080',
    updatedAt: '2026-05-01T00:00:00Z',
  },
  httpsProxy: {
    configured: false,
    displayValue: '',
    updatedAt: null,
  },
  allProxy: {
    configured: false,
    displayValue: '',
    updatedAt: null,
  },
  noProxy: 'localhost,127.0.0.1,::1',
  autoBypassLocal: true,
  needsRestartProjectCount: 2,
  updatedAt: '2026-05-01T00:00:00Z',
  warnings: [],
};

export const handlers = [
  // Auth - login
  http.post(`${BASE_URL}/auth/login`, async ({ request }) => {
    const body = (await request.json()) as { username: string; password: string };
    if (body.username === 'admin' && body.password === 'Admin@123456') {
      return HttpResponse.json({
        success: true,
        retCode: 'SUCCESS',
        retMsg: 'ok',
        data: {
          token: 'mock-jwt-token',
          expiresAt: '2099-01-01T00:00:00Z',
          user: { id: 1, username: 'admin', displayName: 'Administrator', role: 'admin' },
        },
      });
    }
    return HttpResponse.json(
      {
        success: false,
        retCode: 'AUTH_002',
        retMsg: '用户名或密码错误',
        data: null,
      },
      { status: 401 },
    );
  }),

  // Auth - change password
  http.put(`${BASE_URL}/auth/password`, async ({ request }) => {
    const body = (await request.json()) as { oldPassword: string; newPassword: string };
    if (body.oldPassword === 'wrong') {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_003', retMsg: '密码不正确', data: null },
        { status: 400 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // User - profile
  http.get(`${BASE_URL}/user/profile`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: mockAdmin });
  }),

  http.put(`${BASE_URL}/user/profile`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // User - config
  http.get(`${BASE_URL}/user/config`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: { hasGitlabToken: true, gitlabHost: 'https://gitlab.example.com', hasGithubToken: false },
    });
  }),

  http.put(`${BASE_URL}/user/config`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // Admin - users list
  http.get(`${BASE_URL}/admin/users`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const url = new URL(request.url);
    const search = url.searchParams.get('search') || '';
    const filtered = search
      ? mockUsersList.filter(
          (u) => u.username.includes(search) || (u.displayName && u.displayName.includes(search)),
        )
      : mockUsersList;
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        records: filtered,
        totalCount: filtered.length,
        pageNo: 1,
        pageSize: 10,
        pages: 1,
        limit: 10,
        offset: 0,
      },
    });
  }),

  // Admin - create user
  http.post(`${BASE_URL}/admin/users`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: null,
    });
  }),

  // Admin - delete user
  http.delete(`${BASE_URL}/admin/users/:id`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // Admin - reset password
  http.put(`${BASE_URL}/admin/users/:id/reset-password`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // Admin - system config
  http.get(`${BASE_URL}/admin/config`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: [
        {
          key: 'global_concurrency_limit',
          value: '10',
          description: '全局并发上限',
          updatedAt: '2026-05-01T00:00:00Z',
        },
      ],
    });
  }),

  http.put(`${BASE_URL}/admin/config`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as {
      configs: Array<{ key: string; value: string }>;
    };
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: body.configs.map((config) => ({
        ...config,
        description: config.key === 'global_concurrency_limit' ? '全局并发上限' : null,
        updatedAt: '2026-05-02T00:00:00Z',
      })),
    });
  }),

  http.get(`${BASE_URL}/admin/stats`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        totalProjects: mockProjects.length,
        runningServices: mockProjects.filter((project) => project.service_status === 'running').length,
        totalUsers: mockUsersList.length,
        globalConcurrencyLimit: 10,
        globalConcurrencyUsed: 1,
      },
    });
  }),

  // Admin - network proxy
  http.get(`${BASE_URL}/admin/network-proxy`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: mockNetworkProxy,
    });
  }),

  http.put(`${BASE_URL}/admin/network-proxy`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as {
      mode: MockProxyMode;
      httpProxy: { action: 'keep' | 'set' | 'clear' };
      httpsProxy: { action: 'keep' | 'set' | 'clear' };
      allProxy: { action: 'keep' | 'set' | 'clear' };
      noProxy: string;
      autoBypassLocal: boolean;
    };

    const nextSecret = (
      current: MockProxySecret,
      update: { action: 'keep' | 'set' | 'clear' },
      displayValue: string,
    ) => {
      if (update.action === 'keep') return current;
      if (update.action === 'clear') return { configured: false, displayValue: '', updatedAt: null };
      return { configured: true, displayValue, updatedAt: '2026-05-02T00:00:00Z' };
    };

    mockNetworkProxy = {
      ...mockNetworkProxy,
      mode: body.mode,
      version: 'proxy-v2',
      httpProxy: nextSecret(mockNetworkProxy.httpProxy, body.httpProxy, 'http://ne***@proxy.internal:8080'),
      httpsProxy: nextSecret(mockNetworkProxy.httpsProxy, body.httpsProxy, 'http://ht***@proxy.internal:8443'),
      allProxy: nextSecret(mockNetworkProxy.allProxy, body.allProxy, 'socks5://al***@proxy.internal:1080'),
      noProxy: body.noProxy,
      autoBypassLocal: body.autoBypassLocal,
      updatedAt: '2026-05-02T00:00:00Z',
    };

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: mockNetworkProxy,
    });
  }),

  http.post(`${BASE_URL}/admin/network-proxy/test`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        status: 'success',
        targetHost: 'github.com',
        proxyUsed: mockNetworkProxy.mode === 'manual',
        proxySummary: mockNetworkProxy.mode === 'manual' ? 'HTTP proxy' : 'direct',
        durationMs: 42,
        message: '连接成功',
      },
    });
  }),

  // ===== Phase 2: Projects =====

  // Projects - list
  http.get(`${BASE_URL}/projects`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const url = new URL(request.url);
    const search = url.searchParams.get('search') || '';
    const platform = url.searchParams.get('platform') || '';

    let filtered = [...mockProjects];
    if (search) {
      filtered = filtered.filter(
        (p) => p.name.includes(search) || p.git_url.includes(search),
      );
    }
    if (platform) {
      filtered = filtered.filter((p) => p.platform === platform);
    }

    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        records: filtered,
        totalCount: filtered.length,
        pageNo: 1,
        pageSize: 20,
        pages: 1,
        limit: 20,
        offset: 0,
      },
    });
  }),

  // Projects - create
  http.post(`${BASE_URL}/projects`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as { git_url: string; name?: string; description?: string; default_branch?: string };
    const newProject = {
      id: 3,
      name: body.name || 'new-project',
      description: body.description || null,
      git_url: body.git_url,
      platform: body.git_url.includes('github') ? 'github' : 'gitlab',
      platform_host: null,
      namespace: 'group',
      repo_name: 'new-project',
      default_branch: body.default_branch || 'main',
      workflow_template: 'default',
      service_status: 'stopped',
      service_pid: null,
      max_concurrent_agents: 2,
      auto_restart: true,
      member_count: 1,
      my_role: 'owner',
      created_by: 1,
      created_at: '2024-03-01T00:00:00Z',
      updated_at: '2024-03-01T00:00:00Z',
    };
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: newProject,
    });
  }),

  // Projects - get single
  http.get(`${BASE_URL}/projects/:id`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const id = Number(params.id);
    const project = mockProjects.find((p) => p.id === id);
    if (!project) {
      return HttpResponse.json(
        { success: false, retCode: 'NOT_FOUND', retMsg: '项目不存在', data: null },
        { status: 404 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: project,
    });
  }),

  // Projects - update
  http.put(`${BASE_URL}/projects/:id`, async ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const id = Number(params.id);
    const project = mockProjects.find((p) => p.id === id);
    if (!project) {
      return HttpResponse.json(
        { success: false, retCode: 'NOT_FOUND', retMsg: '项目不存在', data: null },
        { status: 404 },
      );
    }
    const body = (await request.json()) as Record<string, unknown>;
    const updated = { ...project, ...body, updated_at: '2024-03-02T00:00:00Z' };
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: updated,
    });
  }),

  // Projects - delete
  http.delete(`${BASE_URL}/projects/:id`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // Projects - start service
  http.post(`${BASE_URL}/projects/:id/start`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        status: 'running',
        pid: 12345,
        started_at: '2024-03-01T10:00:00Z',
        uptime_seconds: 0,
        restart_count: 0,
        error_message: null,
      },
    });
  }),

  // Projects - stop service
  http.post(`${BASE_URL}/projects/:id/stop`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        status: 'stopped',
        pid: null,
        started_at: null,
        uptime_seconds: null,
        restart_count: 0,
        error_message: null,
      },
    });
  }),

  // Projects - restart service
  http.post(`${BASE_URL}/projects/:id/restart`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        status: 'running',
        pid: 12346,
        started_at: '2024-03-01T10:05:00Z',
        uptime_seconds: 0,
        restart_count: 1,
        error_message: null,
      },
    });
  }),

  // Projects - get service status
  http.get(`${BASE_URL}/projects/:id/status`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        status: 'running',
        pid: 12345,
        started_at: '2024-03-01T10:00:00Z',
        uptime_seconds: 3600,
        restart_count: 0,
        error_message: null,
      },
    });
  }),

  // Projects - get members
  http.get(`${BASE_URL}/projects/:id/members`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: mockMembers,
    });
  }),

  // Projects - add member
  http.post(`${BASE_URL}/projects/:id/members`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as { user_id: number; role?: string };
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        user_id: body.user_id,
        username: 'newmember',
        display_name: 'New Member',
        role: body.role || 'member',
        synced_from: null,
        created_at: '2024-03-01T00:00:00Z',
      },
    });
  }),

  // Projects - update member role
  http.put(`${BASE_URL}/projects/:id/members/:userId`, async ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as { role: string };
    const userId = Number(params.userId);
    const member = mockMembers.find((m) => m.user_id === userId);
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: { ...member, role: body.role },
    });
  }),

  // Projects - remove member
  http.delete(`${BASE_URL}/projects/:id/members/:userId`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
  }),

  // Projects - sync members
  http.post(`${BASE_URL}/projects/:id/members/sync`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        added: 2,
        skipped: 1,
        unmatched: ['external-user'],
      },
    });
  }),

  // Projects - get workflow
  http.get(`${BASE_URL}/projects/:id/workflow`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        template_mode: 'default',
        content: '# Default Workflow\n\nThis is the default workflow template.',
        updated_at: '2024-03-01T00:00:00Z',
      },
    });
  }),

  // Projects - update workflow
  http.put(`${BASE_URL}/projects/:id/workflow`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    const body = (await request.json()) as { template_mode: string; content?: string };
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        template_mode: body.template_mode,
        content: body.content || '# Default Workflow\n\nThis is the default workflow template.',
        updated_at: '2024-03-02T00:00:00Z',
      },
    });
  }),

  // Projects - reset workflow
  http.post(`${BASE_URL}/projects/:id/workflow/reset`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) {
      return HttpResponse.json(
        { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
        { status: 401 },
      );
    }
    return HttpResponse.json({
      success: true,
      retCode: 'SUCCESS',
      retMsg: 'ok',
      data: {
        template_mode: 'default',
        content: '# Default Workflow\n\nThis is the default workflow template.',
        updated_at: '2024-03-02T00:00:00Z',
      },
    });
  }),
];
