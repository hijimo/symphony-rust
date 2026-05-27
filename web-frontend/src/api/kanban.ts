import client from './client';
import type { ResponseData } from '../types';
import type { KanbanData, KanbanIssuesData, KanbanParams, KanbanPrsData } from '../types/kanban';

export async function getKanban(
  projectId: number,
  params?: KanbanParams,
  options?: { noCache?: boolean },
): Promise<KanbanData> {
  const requestParams = {
    ...params,
    ...(options?.noCache ? { no_cache: true } : {}),
  };

  const res = await client.get<ResponseData<KanbanData>>(
    `/projects/${projectId}/kanban`,
    {
      params: requestParams,
    },
  );
  return res.data.data;
}

export async function getKanbanIssues(
  projectId: number,
  params?: KanbanParams,
  options?: { noCache?: boolean },
): Promise<KanbanIssuesData> {
  const requestParams = {
    ...params,
    ...(options?.noCache ? { no_cache: true } : {}),
  };

  const res = await client.get<ResponseData<KanbanIssuesData>>(
    `/projects/${projectId}/kanban/issues`,
    { params: requestParams },
  );
  return res.data.data;
}

export async function getKanbanPrs(
  projectId: number,
  options?: { noCache?: boolean },
): Promise<KanbanPrsData> {
  const requestParams = options?.noCache ? { no_cache: true } : {};

  const res = await client.get<ResponseData<KanbanPrsData>>(
    `/projects/${projectId}/kanban/prs`,
    { params: requestParams },
  );
  return res.data.data;
}
