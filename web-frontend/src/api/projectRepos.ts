import client from './client';
import { camelToSnakeKeys, snakeToCamelKeys } from './caseTransform';
import type { ResponseData } from '../types';

export type ProjectRepoMode = 'read' | 'write';
export type ProjectRepoCheckout = 'full' | 'partial';
export type ProjectRepoStatus = 'active' | 'invalid';

export interface ProjectRepo {
  id: number;
  alias: string;
  linkedProjectId: number | null;
  linkedProjectName: string | null;
  namespace: string | null;
  repoName: string | null;
  defaultBranch: string | null;
  resolvedDefaultBranch: string;
  mode: ProjectRepoMode;
  checkout: ProjectRepoCheckout;
  mergeAfter: string[];
  status: ProjectRepoStatus;
  statusReason: string | null;
  writeGrantedBy: number | null;
}

export interface ProjectRepoInput {
  alias: string;
  linkedProjectId?: number;
  gitUrl?: string;
  defaultBranch?: string;
  mode: ProjectRepoMode;
  checkout: ProjectRepoCheckout;
  mergeAfter: string[];
}

export interface ProjectRepoCandidate {
  id: number;
  name: string;
}

export async function getProjectRepos(projectId: number): Promise<ProjectRepo[]> {
  const res = await client.get<ResponseData<ProjectRepo[]>>(`/projects/${projectId}/repos`);
  return snakeToCamelKeys<ProjectRepo[]>(res.data.data ?? []);
}

/** Full replace of the project's related repositories. Request body is sent as snake_case. */
export async function updateProjectRepos(
  projectId: number,
  repos: ProjectRepoInput[],
): Promise<ProjectRepo[]> {
  const res = await client.put<ResponseData<ProjectRepo[]>>(
    `/projects/${projectId}/repos`,
    camelToSnakeKeys({ repos }),
  );
  return snakeToCamelKeys<ProjectRepo[]>(res.data.data ?? []);
}

export async function getProjectRepoCandidates(
  projectId: number,
): Promise<ProjectRepoCandidate[]> {
  const res = await client.get<ResponseData<ProjectRepoCandidate[]>>(
    `/projects/${projectId}/repos/candidates`,
  );
  return res.data.data ?? [];
}
