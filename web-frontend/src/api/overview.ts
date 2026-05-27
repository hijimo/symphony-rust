import client from './client';
import type { ResponseData } from '../types';
import type {
  OverviewIssuesResponse,
  OverviewParams,
  OverviewPrsResponse,
} from '../types/overview';

export async function getOverviewIssues(
  params?: OverviewParams,
  signal?: AbortSignal,
): Promise<OverviewIssuesResponse> {
  const res = await client.get<ResponseData<OverviewIssuesResponse>>(
    '/overview/kanban/issues',
    { params, signal },
  );
  return res.data.data;
}

export async function getOverviewPrs(
  params?: OverviewParams,
  signal?: AbortSignal,
): Promise<OverviewPrsResponse> {
  const res = await client.get<ResponseData<OverviewPrsResponse>>(
    '/overview/kanban/prs',
    { params, signal },
  );
  return res.data.data;
}
