import client from './client';
import type { ResponseData } from '../types';
import type {
  CreateIssueRequest,
  IssueDetail,
  MergeRequestSummary,
  MergeRequestDetail,
} from '../types/issue';

export async function createIssue(
  projectId: number,
  data: CreateIssueRequest,
): Promise<IssueDetail> {
  const res = await client.post<ResponseData<IssueDetail>>(
    `/projects/${projectId}/issues`,
    data,
  );
  return res.data.data;
}

export async function getIssue(
  projectId: number,
  iid: number,
): Promise<IssueDetail> {
  const res = await client.get<ResponseData<IssueDetail>>(
    `/projects/${projectId}/issues/${iid}`,
  );
  return res.data.data;
}

export async function getIssueMrs(
  projectId: number,
  iid: number,
): Promise<MergeRequestSummary[]> {
  const res = await client.get<ResponseData<MergeRequestSummary[]>>(
    `/projects/${projectId}/issues/${iid}/mrs`,
  );
  return res.data.data;
}

export async function getMergeRequest(
  projectId: number,
  iid: number,
): Promise<MergeRequestDetail> {
  const res = await client.get<ResponseData<MergeRequestDetail>>(
    `/projects/${projectId}/mrs/${iid}`,
  );
  return res.data.data;
}
