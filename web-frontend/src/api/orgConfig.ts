import client from './client';
import type { ResponseData } from '../types';
import type { OrgConfigStatus, ProjectOrgConfig } from '../types/orgConfig';

export async function getOrgConfig(): Promise<OrgConfigStatus> {
  const res = await client.get<ResponseData<OrgConfigStatus>>('/admin/org-config');
  return res.data.data;
}

export async function syncOrgConfig(): Promise<OrgConfigStatus> {
  const res = await client.post<ResponseData<OrgConfigStatus>>('/admin/org-config/sync', {});
  return res.data.data;
}

export async function getProjectOrgConfig(projectId: number): Promise<ProjectOrgConfig> {
  const res = await client.get<ResponseData<ProjectOrgConfig>>(`/projects/${projectId}/org-config`);
  return res.data.data;
}

/** `null` follows the configured ref. */
export async function setProjectOrgPin(projectId: number, pin: string | null): Promise<ProjectOrgConfig> {
  const res = await client.put<ResponseData<ProjectOrgConfig>>(`/projects/${projectId}/org-config`, { pin });
  return res.data.data;
}
