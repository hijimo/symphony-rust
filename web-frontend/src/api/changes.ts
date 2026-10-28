import client from './client';
import type { ResponseData } from '../types';
import type {
  ContextStatus,
  DocTemplate,
  DocType,
  FileContent,
  FlowView,
  GateAction,
  SearchHit,
  StageKanban,
  Track,
  TreeEntry,
  ValidationIssue,
} from '../types/changes';

const base = (projectId: number) => `/projects/${projectId}/changes`;

export interface StartFlowBody {
  track: Track;
  issueIid?: number;
  title?: string;
  summary?: string;
  initialContent?: string;
}

export async function startFlow(projectId: number, body: StartFlowBody): Promise<FlowView> {
  const res = await client.post<ResponseData<FlowView>>(base(projectId), body, { timeout: 120000 });
  return res.data.data;
}

export async function getFlow(projectId: number, iid: number): Promise<FlowView> {
  const res = await client.get<ResponseData<FlowView>>(`${base(projectId)}/${iid}`);
  return res.data.data;
}

export async function ensureWorkspace(
  projectId: number,
  iid: number,
): Promise<{ context: ContextStatus; writableDir: string; multiRepo: boolean; primaryDir: string }> {
  const res = await client.post(`${base(projectId)}/${iid}/workspace`, {}, { timeout: 600000 });
  return res.data.data;
}

export async function mergeMain(projectId: number, iid: number): Promise<void> {
  await client.post(`${base(projectId)}/${iid}/workspace/merge-main`, {}, { timeout: 120000 });
}

export async function listTree(projectId: number, iid: number, path = ''): Promise<TreeEntry[]> {
  const res = await client.get<ResponseData<TreeEntry[]>>(`${base(projectId)}/${iid}/tree`, {
    params: { path },
  });
  return res.data.data;
}

export async function readFile(projectId: number, iid: number, path: string): Promise<FileContent> {
  const res = await client.get<ResponseData<FileContent>>(`${base(projectId)}/${iid}/file`, {
    params: { path },
  });
  return res.data.data;
}

export async function readFileAt(projectId: number, iid: number, path: string, rev: string): Promise<string | null> {
  const res = await client.get(`${base(projectId)}/${iid}/file`, { params: { path, rev } });
  return res.data.data.content ?? null;
}

export async function writeFile(
  projectId: number,
  iid: number,
  body: { path: string; content: string; baseVersion: string | null; commit?: boolean },
): Promise<{ version: string; commitSha: string | null }> {
  const res = await client.put(`${base(projectId)}/${iid}/file`, body, { timeout: 60000 });
  return res.data.data;
}

export async function search(
  projectId: number,
  iid: number,
  q: string,
  mode: 'file' | 'content',
): Promise<SearchHit[]> {
  const res = await client.get<ResponseData<SearchHit[]>>(`${base(projectId)}/${iid}/search`, {
    params: { q, mode },
  });
  return res.data.data;
}

export async function getDiff(projectId: number, iid: number): Promise<string> {
  const res = await client.get(`${base(projectId)}/${iid}/diff`);
  return res.data.data.diff;
}

export async function listTemplates(projectId: number, iid: number): Promise<DocTemplate[]> {
  const res = await client.get<ResponseData<DocTemplate[]>>(`${base(projectId)}/${iid}/templates`);
  return res.data.data;
}

export async function validateDoc(projectId: number, iid: number, doc: DocType): Promise<ValidationIssue[]> {
  const res = await client.post<ResponseData<ValidationIssue[]>>(
    `${base(projectId)}/${iid}/docs/${doc}/validate`,
    {},
  );
  return res.data.data;
}

export async function submitDoc(projectId: number, iid: number, doc: DocType): Promise<FlowView> {
  const res = await client.post<ResponseData<FlowView>>(
    `${base(projectId)}/${iid}/docs/${doc}/submit`,
    {},
    { timeout: 60000 },
  );
  return res.data.data;
}

export async function decideDoc(
  projectId: number,
  iid: number,
  doc: DocType,
  action: GateAction,
  comment?: string,
): Promise<FlowView> {
  const res = await client.post<ResponseData<FlowView>>(
    `${base(projectId)}/${iid}/docs/${doc}/decision`,
    { action, comment },
    { timeout: 120000 },
  );
  return res.data.data;
}

export async function retryMerge(projectId: number, iid: number): Promise<FlowView> {
  const res = await client.post<ResponseData<FlowView>>(`${base(projectId)}/${iid}/merge`, {}, { timeout: 120000 });
  return res.data.data;
}

export async function revertFlow(
  projectId: number,
  iid: number,
  body: { to: DocType; implBranch?: string; codeMrIid?: number },
): Promise<FlowView> {
  const res = await client.post<ResponseData<FlowView>>(`${base(projectId)}/${iid}/revert`, body, {
    timeout: 600000,
  });
  return res.data.data;
}

export async function retryDraft(projectId: number, iid: number, doc: DocType): Promise<void> {
  await client.post(`${base(projectId)}/${iid}/docs/${doc}/draft/retry`, {});
}

export async function manualDraft(projectId: number, iid: number, doc: DocType): Promise<void> {
  await client.post(`${base(projectId)}/${iid}/docs/${doc}/draft/manual`, {});
}

export async function wsTicket(projectId: number, iid: number): Promise<string> {
  const res = await client.post(`${base(projectId)}/${iid}/ws-ticket`, {});
  return res.data.data.ticket;
}

export function wsUrl(projectId: number, iid: number, ticket: string): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${window.location.host}/api${base(projectId)}/${iid}/ws?ticket=${encodeURIComponent(ticket)}`;
}

export async function getStageKanban(
  projectId: number,
  params?: { search?: string; author?: string },
): Promise<StageKanban> {
  const res = await client.get<ResponseData<StageKanban>>(`/projects/${projectId}/kanban/stages`, {
    params,
  });
  return res.data.data;
}
