import client from './client';

export interface ConcurrencyStatus {
  global_max: number;
  global_active: number;
  utilization_percent: number;
  projects: ProjectConcurrencyInfo[];
  data_freshness_seconds: number;
}

export interface ProjectConcurrencyInfo {
  project_id: number;
  project_name: string;
  active_agents: number;
  max_agents: number | null;
  queued_tasks: number;
  service_status: string;
}

export interface ConcurrencyConfigResponse {
  global_max: number;
  previous_value: number;
}

export interface ProjectConcurrencyDetail {
  project_id: number;
  project_name: string;
  active_agents: number;
  max_agents: number | null;
  queued_tasks: number;
  today_started: number;
  today_completed: number;
  avg_duration_seconds: number | null;
}

export interface SseTicketResponse {
  ticket: string;
  expires_at: string;
}

export interface ValidateTokenRequest {
  platform: string;
  token: string;
}

export interface ValidateTokenResponse {
  valid: boolean;
  username: string | null;
  scopes: string[];
  error: string | null;
}

export interface Contributor {
  username: string;
  display_name: string;
  avatar_url: string;
  recent_issue_count: number;
  recent_mr_count: number;
  is_bot: boolean;
  logical_author: boolean;
}

export interface ContributorsResponse {
  contributors: Contributor[];
  scope: string;
}

export async function getGlobalConcurrency(): Promise<ConcurrencyStatus> {
  const resp = await client.get('/admin/concurrency');
  return resp.data.data;
}

export async function updateConcurrencyConfig(params: {
  global_max: number;
  expected_previous?: number;
}): Promise<ConcurrencyConfigResponse> {
  const resp = await client.put('/admin/concurrency/config', params);
  return resp.data.data;
}

export async function getProjectConcurrency(
  projectId: number
): Promise<ProjectConcurrencyDetail> {
  const resp = await client.get(`/projects/${projectId}/concurrency`);
  return resp.data.data;
}

export async function updateProjectConcurrency(
  projectId: number,
  maxAgents: number
): Promise<void> {
  await client.put(`/projects/${projectId}/concurrency`, {
    max_agents: maxAgents,
  });
}

export async function getSseTicket(): Promise<SseTicketResponse> {
  const resp = await client.post('/admin/concurrency/events/ticket');
  return resp.data.data;
}

export async function validateToken(
  req: ValidateTokenRequest
): Promise<ValidateTokenResponse> {
  const resp = await client.post('/user/config/validate-token', req);
  return resp.data.data;
}

export async function getContributors(
  projectId: number
): Promise<ContributorsResponse> {
  const resp = await client.get(`/projects/${projectId}/contributors`);
  return resp.data.data;
}
