import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import ProjectReposEditor, { validateRepoDrafts } from '../ProjectReposEditor';
import type { RepoDraft } from '../ProjectReposEditor';

const ok = (data: unknown) =>
  HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data });

const linkedRepo = {
  id: 1,
  alias: 'backend',
  linkedProjectId: 3,
  linkedProjectName: 'jxy',
  namespace: null,
  repoName: null,
  defaultBranch: null,
  resolvedDefaultBranch: 'pre',
  mode: 'read',
  checkout: 'partial',
  mergeAfter: [],
  status: 'active',
  statusReason: null,
  writeGrantedBy: null,
};

const invalidRepo = {
  ...linkedRepo,
  id: 2,
  alias: 'sdk',
  linkedProjectId: null,
  linkedProjectName: null,
  namespace: 'group',
  repoName: 'sdk',
  defaultBranch: 'master',
  resolvedDefaultBranch: 'master',
  mergeAfter: ['backend'],
  status: 'invalid',
  statusReason: '保存人已不是主项目成员',
};

function setupHandlers(repos: unknown[] = [linkedRepo, invalidRepo]) {
  const state = { repos, putBodies: [] as unknown[], getCount: 0 };
  server.use(
    http.get('*/api/projects/1/repos', () => {
      state.getCount += 1;
      return ok(state.repos);
    }),
    http.get('*/api/projects/1/repos/candidates', () => ok([{ id: 5, name: 'other-project' }])),
    http.put('*/api/projects/1/repos', async ({ request }) => {
      const body = await request.json();
      state.putBodies.push(body);
      return ok(state.repos);
    }),
  );
  return state;
}

function renderEditor(isCreator = true) {
  return render(
    <ThemeProvider theme={theme}>
      <ProjectReposEditor
        projectId={1}
        primaryGitUrl="https://gitlab.com/group/my-project.git"
        isCreator={isCreator}
      />
    </ThemeProvider>,
  );
}

describe('ProjectReposEditor', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('renders title, help text and repo rows', async () => {
    setupHandlers();
    renderEditor();
    expect(screen.getByRole('heading', { name: '关联仓库' })).toBeInTheDocument();
    expect(
      screen.getByText('关联仓库会被检出到 Issue 工作区供 agent 参考；默认只读。'),
    ).toBeInTheDocument();

    const rows = await screen.findAllByTestId('project-repo-row');
    expect(rows).toHaveLength(2);
    const backend = within(rows[0]);
    expect(backend.getByLabelText(/别名/)).toHaveValue('backend');
    expect(backend.getByRole('radio', { name: 'Symphony 项目' })).toBeChecked();
    expect(backend.getByText('jxy')).toBeInTheDocument();
    // Empty branch shows inherited value as placeholder
    expect(backend.getByLabelText(/默认分支/)).toHaveAttribute('placeholder', '继承：pre');

    const sdk = within(rows[1]);
    expect(sdk.getByRole('radio', { name: '手动填写仓库' })).toBeChecked();
    expect(sdk.getByLabelText(/Git URL/)).toHaveValue('https://gitlab.com/group/sdk.git');
  });

  it('highlights invalid rows with status reason linked via aria-describedby', async () => {
    setupHandlers();
    renderEditor();
    const rows = await screen.findAllByTestId('project-repo-row');
    expect(rows[0]).toHaveAttribute('data-invalid', 'false');
    expect(rows[1]).toHaveAttribute('data-invalid', 'true');
    const group = screen.getByRole('group', { name: '关联仓库 sdk' });
    expect(group).toHaveAccessibleDescription('保存人已不是主项目成员');
  });

  it('shows write-mode hint and disables 可写 for non-creator', async () => {
    setupHandlers([linkedRepo]);
    const user = userEvent.setup();
    renderEditor(false);
    await screen.findAllByTestId('project-repo-row');
    expect(screen.getByText(/可写仅项目创建者可设置/)).toBeInTheDocument();

    await user.click(screen.getByRole('combobox', { name: /读写模式/ }));
    const writeOption = await screen.findByRole('option', { name: '可写' });
    expect(writeOption).toHaveAttribute('aria-disabled', 'true');
  });

  it('allows creator to pick 可写', async () => {
    setupHandlers([linkedRepo]);
    const user = userEvent.setup();
    renderEditor(true);
    await screen.findAllByTestId('project-repo-row');
    await user.click(screen.getByRole('combobox', { name: /读写模式/ }));
    const writeOption = await screen.findByRole('option', { name: '可写' });
    expect(writeOption).not.toHaveAttribute('aria-disabled', 'true');
  });

  it('shows validation errors and does not PUT', async () => {
    const state = setupHandlers([]);
    const user = userEvent.setup();
    renderEditor();
    await screen.findByText('暂无关联仓库');

    await user.click(screen.getByRole('button', { name: '添加关联仓库' }));
    const row = within(screen.getByTestId('project-repo-row'));
    await user.type(row.getByLabelText(/别名/), 'Bad_Alias');
    await user.click(row.getByRole('radio', { name: '手动填写仓库' }));
    await user.type(row.getByLabelText(/Git URL/), 'not-a-url');
    await user.click(screen.getByRole('button', { name: '保存关联仓库' }));

    expect(await row.findByText('别名只能包含小写字母、数字和 -')).toBeInTheDocument();
    expect(row.getByText('仓库地址格式不正确')).toBeInTheDocument();
    expect(row.getByText('手动填写仓库时必须指定默认分支')).toBeInTheDocument();
    expect(row.getByLabelText(/别名/)).toHaveAccessibleDescription(
      '别名只能包含小写字母、数字和 -',
    );
    expect(state.putBodies).toHaveLength(0);
  });

  it('saves with snake_case payload and refreshes list', async () => {
    const state = setupHandlers([linkedRepo]);
    const user = userEvent.setup();
    renderEditor();
    await screen.findAllByTestId('project-repo-row');
    expect(state.getCount).toBe(1);

    await user.click(screen.getByRole('button', { name: '添加关联仓库' }));
    const rows = screen.getAllByTestId('project-repo-row');
    const row = within(rows[1]);
    await user.type(row.getByLabelText(/别名/), 'sdk');
    await user.click(row.getByRole('radio', { name: '手动填写仓库' }));
    await user.type(row.getByLabelText(/Git URL/), 'https://gitlab.com/group/sdk.git');
    await user.type(row.getByLabelText(/默认分支/), 'master');
    await user.click(row.getByRole('combobox', { name: /检出方式/ }));
    await user.click(await screen.findByRole('option', { name: 'full' }));
    await user.click(row.getByRole('combobox', { name: /合并依赖/ }));
    await user.click(await screen.findByRole('option', { name: 'backend' }));
    await user.keyboard('{Escape}');

    await user.click(screen.getByRole('button', { name: '保存关联仓库' }));
    await waitFor(() => expect(state.putBodies).toHaveLength(1));
    expect(state.putBodies[0]).toEqual({
      repos: [
        {
          alias: 'backend',
          linked_project_id: 3,
          mode: 'read',
          checkout: 'partial',
          merge_after: [],
        },
        {
          alias: 'sdk',
          git_url: 'https://gitlab.com/group/sdk.git',
          default_branch: 'master',
          mode: 'read',
          checkout: 'full',
          merge_after: ['backend'],
        },
      ],
    });
    expect(await screen.findByText('关联仓库已保存')).toBeInTheDocument();
    expect(state.getCount).toBe(2);
  });

  it('shows backend retMsg on save error', async () => {
    setupHandlers([linkedRepo]);
    server.use(
      http.put('*/api/projects/1/repos', () =>
        HttpResponse.json(
          { success: false, retCode: 'FORBIDDEN', retMsg: '只有项目创建者可以设置可写', data: null },
          { status: 403 },
        ),
      ),
    );
    const user = userEvent.setup();
    renderEditor();
    await screen.findAllByTestId('project-repo-row');
    await user.click(screen.getByRole('button', { name: '保存关联仓库' }));
    expect(await screen.findByText('只有项目创建者可以设置可写')).toBeInTheDocument();
  });

  it('removes a row', async () => {
    setupHandlers();
    const user = userEvent.setup();
    renderEditor();
    await screen.findAllByTestId('project-repo-row');
    await user.click(screen.getByRole('button', { name: '删除关联仓库 backend' }));
    expect(screen.getAllByTestId('project-repo-row')).toHaveLength(1);
  });
});

describe('validateRepoDrafts', () => {
  const base: RepoDraft = {
    key: 'a',
    alias: 'backend',
    source: 'linked',
    linkedProjectId: 3,
    gitUrl: '',
    defaultBranch: '',
    mode: 'read',
    checkout: 'partial',
    mergeAfter: [],
  };

  it('rejects primary, duplicates and missing linked project', () => {
    const errs = validateRepoDrafts([
      { ...base, key: 'a', alias: 'primary' },
      { ...base, key: 'b', alias: 'dup' },
      { ...base, key: 'c', alias: 'dup', linkedProjectId: '' },
    ]);
    expect(errs.a.alias).toMatch(/primary/);
    expect(errs.b.alias).toBe('别名不能重复');
    expect(errs.c.alias).toBe('别名不能重复');
    expect(errs.c.linkedProjectId).toBe('请选择 Symphony 项目');
  });

  it('rejects self or unknown merge_after and accepts primary', () => {
    const errs = validateRepoDrafts([
      { ...base, key: 'a', alias: 'x', mergeAfter: ['primary'] },
      { ...base, key: 'b', alias: 'y', mergeAfter: ['y', 'zzz'] },
    ]);
    expect(errs.a).toBeUndefined();
    expect(errs.b.mergeAfter).toContain('y, zzz');
  });
});
