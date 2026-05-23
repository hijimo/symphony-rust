import client from './client';
import type {
  ResponseData,
  ProjectMember,
  SyncResult,
  AddMemberParams,
  UpdateMemberRoleParams,
} from '../types';

export async function getMembers(projectId: number): Promise<ProjectMember[]> {
  const res = await client.get<ResponseData<ProjectMember[]>>(
    `/projects/${projectId}/members`,
  );
  return res.data.data;
}

export async function addMember(
  projectId: number,
  data: AddMemberParams,
): Promise<ProjectMember> {
  const res = await client.post<ResponseData<ProjectMember>>(
    `/projects/${projectId}/members`,
    data,
  );
  return res.data.data;
}

export async function updateMemberRole(
  projectId: number,
  userId: number,
  data: UpdateMemberRoleParams,
): Promise<ProjectMember> {
  const res = await client.put<ResponseData<ProjectMember>>(
    `/projects/${projectId}/members/${userId}`,
    data,
  );
  return res.data.data;
}

export async function removeMember(
  projectId: number,
  userId: number,
): Promise<void> {
  await client.delete<ResponseData>(`/projects/${projectId}/members/${userId}`);
}

export async function syncMembers(projectId: number): Promise<SyncResult> {
  const res = await client.post<ResponseData<SyncResult>>(
    `/projects/${projectId}/members/sync`,
  );
  return res.data.data;
}
