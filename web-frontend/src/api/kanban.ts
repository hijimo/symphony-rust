import client from './client';
import type { ResponseData } from '../types';
import type { KanbanData, KanbanParams } from '../types/kanban';

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
  // Phase 3 entities already use snake_case from backend, no transform needed
  return res.data.data;
}
