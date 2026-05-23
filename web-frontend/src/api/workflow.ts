import client from './client';
import type {
  ResponseData,
  WorkflowData,
  UpdateWorkflowParams,
} from '../types';

export async function getWorkflow(projectId: number): Promise<WorkflowData> {
  const res = await client.get<ResponseData<WorkflowData>>(
    `/projects/${projectId}/workflow`,
  );
  return res.data.data;
}

export async function updateWorkflow(
  projectId: number,
  data: UpdateWorkflowParams,
): Promise<WorkflowData> {
  const res = await client.put<ResponseData<WorkflowData>>(
    `/projects/${projectId}/workflow`,
    data,
  );
  return res.data.data;
}

export async function resetWorkflow(projectId: number): Promise<WorkflowData> {
  const res = await client.post<ResponseData<WorkflowData>>(
    `/projects/${projectId}/workflow/reset`,
  );
  return res.data.data;
}
