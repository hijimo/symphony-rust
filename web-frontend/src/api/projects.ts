import client from './client';
import { camelToSnakeKeys } from './caseTransform';
import type {
  ResponseData,
  PaginationData,
  Project,
  ServiceStatusData,
  UpdateProjectParams,
  ProjectPlatform,
  ServiceStatus,
} from '../types';

export interface GetProjectsParams {
  pageNo: number;
  pageSize: number;
  platform?: ProjectPlatform;
  status?: ServiceStatus;
  search?: string;
}

export interface CreateProjectParams {
  git_url: string;
  name?: string;
  description?: string;
  default_branch?: string;
  workflow_template?: 'default' | 'custom';
  workflow_content?: string;
}

export async function getProjects(params: GetProjectsParams): Promise<PaginationData<Project>> {
  // Backend expects snake_case query params: page_no, page_size
  const queryParams = {
    page_no: params.pageNo,
    page_size: params.pageSize,
    platform: params.platform,
    status: params.status,
    search: params.search,
  };
  const res = await client.get<ResponseData<PaginationData<Project>>>('/projects', { params: queryParams });
  // Backend returns camelCase fields; transform to snake_case for frontend types
  const data = res.data.data;
  return {
    ...data,
    records: camelToSnakeKeys<Project[]>(data.records),
  };
}

export async function createProject(data: CreateProjectParams): Promise<Project> {
  const res = await client.post<ResponseData<Project>>('/projects', data);
  return camelToSnakeKeys<Project>(res.data.data);
}

export async function getProject(projectId: number): Promise<Project> {
  const res = await client.get<ResponseData<Project>>(`/projects/${projectId}`);
  return camelToSnakeKeys<Project>(res.data.data);
}

export async function updateProject(
  projectId: number,
  data: UpdateProjectParams,
): Promise<Project> {
  const res = await client.put<ResponseData<Project>>(
    `/projects/${projectId}`,
    data,
  );
  return camelToSnakeKeys<Project>(res.data.data);
}

export async function deleteProject(projectId: number): Promise<void> {
  await client.delete<ResponseData>(`/projects/${projectId}`);
}

export async function getServiceStatus(
  projectId: number,
): Promise<ServiceStatusData> {
  const res = await client.get<ResponseData<ServiceStatusData>>(
    `/projects/${projectId}/status`,
  );
  return camelToSnakeKeys<ServiceStatusData>(res.data.data);
}

export async function startService(
  projectId: number,
): Promise<ServiceStatusData> {
  const res = await client.post<ResponseData<ServiceStatusData>>(
    `/projects/${projectId}/start`,
  );
  return camelToSnakeKeys<ServiceStatusData>(res.data.data);
}

export async function stopService(
  projectId: number,
): Promise<ServiceStatusData> {
  const res = await client.post<ResponseData<ServiceStatusData>>(
    `/projects/${projectId}/stop`,
  );
  return camelToSnakeKeys<ServiceStatusData>(res.data.data);
}

export async function restartService(
  projectId: number,
): Promise<ServiceStatusData> {
  const res = await client.post<ResponseData<ServiceStatusData>>(
    `/projects/${projectId}/restart`,
  );
  return camelToSnakeKeys<ServiceStatusData>(res.data.data);
}
