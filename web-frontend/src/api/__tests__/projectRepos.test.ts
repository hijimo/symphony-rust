import { describe, it, expect, beforeEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../test/mocks/server';
import {
  getProjectRepos,
  getProjectRepoCandidates,
  updateProjectRepos,
} from '../projectRepos';

const ok = (data: unknown) =>
  HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data });

describe('projectRepos api', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('getProjectRepos normalizes snake_case keys to camelCase', async () => {
    server.use(
      http.get('*/api/projects/7/repos', () =>
        ok([
          {
            id: 1,
            alias: 'backend',
            linked_project_id: 3,
            linked_project_name: 'jxy',
            namespace: null,
            repo_name: null,
            default_branch: null,
            resolved_default_branch: 'pre',
            mode: 'read',
            checkout: 'partial',
            merge_after: ['primary'],
            status: 'active',
            status_reason: null,
            write_granted_by: null,
          },
        ]),
      ),
    );
    const repos = await getProjectRepos(7);
    expect(repos[0]).toMatchObject({
      alias: 'backend',
      linkedProjectId: 3,
      linkedProjectName: 'jxy',
      resolvedDefaultBranch: 'pre',
      mergeAfter: ['primary'],
    });
  });

  it('getProjectRepos keeps camelCase responses as-is', async () => {
    server.use(
      http.get('*/api/projects/7/repos', () =>
        ok([{ id: 2, alias: 'sdk', linkedProjectId: null, resolvedDefaultBranch: 'main' }]),
      ),
    );
    const repos = await getProjectRepos(7);
    expect(repos[0].linkedProjectId).toBeNull();
    expect(repos[0].resolvedDefaultBranch).toBe('main');
  });

  it('updateProjectRepos sends snake_case body via PUT', async () => {
    let body: unknown;
    let method = '';
    server.use(
      http.put('*/api/projects/7/repos', async ({ request }) => {
        method = request.method;
        body = await request.json();
        return ok([]);
      }),
    );
    await updateProjectRepos(7, [
      {
        alias: 'sdk',
        gitUrl: 'https://gitlab.com/g/sdk.git',
        defaultBranch: 'main',
        mode: 'read',
        checkout: 'full',
        mergeAfter: ['primary'],
      },
    ]);
    expect(method).toBe('PUT');
    expect(body).toEqual({
      repos: [
        {
          alias: 'sdk',
          git_url: 'https://gitlab.com/g/sdk.git',
          default_branch: 'main',
          mode: 'read',
          checkout: 'full',
          merge_after: ['primary'],
        },
      ],
    });
  });

  it('updateProjectRepos rejects with backend retMsg on 403', async () => {
    server.use(
      http.put('*/api/projects/7/repos', () =>
        HttpResponse.json(
          { success: false, retCode: 'FORBIDDEN', retMsg: '只有项目创建者可以设置可写', data: null },
          { status: 403 },
        ),
      ),
    );
    await expect(updateProjectRepos(7, [])).rejects.toThrow('只有项目创建者可以设置可写');
  });

  it('getProjectRepoCandidates returns list', async () => {
    server.use(
      http.get('*/api/projects/7/repos/candidates', () => ok([{ id: 3, name: 'jxy' }])),
    );
    expect(await getProjectRepoCandidates(7)).toEqual([{ id: 3, name: 'jxy' }]);
  });
});
