// Phase 4 - Concurrency monitoring types
// Fields use camelCase (transformed from backend snake_case via API response)

export interface ProjectConcurrencyInfo {
  projectId: number;
  projectName: string;
  maxAgents: number;
  activeAgents: number;
  queuedTasks: number;
  status: 'running' | 'stopped' | 'error';
  lastHeartbeat: string | null;
}

export interface GlobalConcurrencyStatus {
  globalMax: number;
  globalActive: number;
  globalQueued: number;
  utilizationPercent: number;
  projects: ProjectConcurrencyInfo[];
  throttledProjects: number[];
  updatedAt: string;
  dataFreshnessSeconds: number;
}

export interface UpdateConcurrencyConfigRequest {
  globalMax: number;
  expectedPrevious?: number;
}

export interface UpdateConcurrencyConfigResponse {
  globalMax: number;
  previousValue: number;
  effectiveImmediately: boolean;
}

export interface UpdateProjectConcurrencyRequest {
  maxAgents: number;
  expectedPrevious?: number;
}

export interface UpdateProjectConcurrencyResponse {
  projectId: number;
  maxAgents: number;
  previousValue: number;
  effectiveImmediately: boolean;
}

export interface ActiveAgentInfo {
  agentId: string;
  issueIid: number;
  issueTitle: string;
  startedAt: string;
  elapsedSeconds: number;
}

export interface ConcurrencyHistory {
  peakToday: number;
  totalTasksToday: number;
  avgTaskDurationSeconds: number;
}

export interface ProjectConcurrencyDetail {
  projectId: number;
  maxAgents: number;
  activeAgents: number;
  queuedTasks: number;
  isThrottled: boolean;
  throttleReason: string | null;
  agents: ActiveAgentInfo[];
  history: ConcurrencyHistory;
  lastHeartbeat: string | null;
}

export interface SseTicketResponse {
  ticket: string;
  expiresInSeconds: number;
}

// SSE event types
export type ConcurrencyEventType =
  | 'snapshot'
  | 'agent_started'
  | 'agent_completed'
  | 'throttle_changed'
  | 'config_changed'
  | 'heartbeat';

export interface ConcurrencySnapshotEvent {
  globalActive: number;
  globalMax: number;
  globalQueued: number;
  projects: ProjectConcurrencyInfo[];
}

export interface AgentStartedEvent {
  projectId: number;
  agentId: string;
  issueIid: number;
  timestamp: string;
}

export interface AgentCompletedEvent {
  projectId: number;
  agentId: string;
  issueIid: number;
  durationSeconds: number;
  timestamp: string;
}

export interface ThrottleChangedEvent {
  projectId: number;
  isThrottled: boolean;
  reason: string | null;
  timestamp: string;
}

export interface ConfigChangedEvent {
  globalMax: number;
  changedBy: string;
  timestamp: string;
}

export interface HeartbeatEvent {
  timestamp: string;
}
