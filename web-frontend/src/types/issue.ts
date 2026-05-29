// Phase 3 - Issue & MR types (snake_case per API spec, data from GitLab/GitHub)

export interface PlatformUser {
  username: string;
  display_name: string | null;
  avatar_url: string | null;
}

export interface Reviewer {
  user: PlatformUser;
  state: 'pending' | 'approved' | 'changes_requested';
}

// ==================== Issue Types ====================

export interface CreateIssueRequest {
  title: string;
  description?: string;
  labels?: string[];
  assignee?: string;
}

export interface AIGenerateRequest {
  prompt: string;
  title?: string;
  context?: string;
}

export interface IssueDetail {
  iid: number;
  title: string;
  description: string | null;
  state: 'opened' | 'closed';
  labels: string[];
  author: PlatformUser;
  assignees: PlatformUser[];
  milestone: string | null;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  web_url: string;
  comment_count: number;
  related_mrs: MergeRequestSummary[];
}

export interface MergeRequestSummary {
  iid: number;
  title: string;
  state: 'opened' | 'closed' | 'merged';
  author: PlatformUser;
  web_url: string;
}

export interface IssueSummary {
  iid: number;
  title: string;
  state: 'opened' | 'closed';
  web_url: string;
}

// ==================== Merge Request Types ====================

export interface MergeRequestDetail {
  iid: number;
  title: string;
  description: string | null;
  state: 'opened' | 'closed' | 'merged';
  author: PlatformUser;
  source_branch: string;
  target_branch: string;
  ci_status: 'pending' | 'running' | 'success' | 'failed' | 'canceled' | null;
  ci_web_url: string | null;
  review_status: 'pending' | 'approved' | 'changes_requested' | null;
  reviewers: Reviewer[];
  merge_status: 'can_be_merged' | 'cannot_be_merged' | 'checking' | 'unchecked';
  related_issues: IssueSummary[];
  additions: number;
  deletions: number;
  changed_files: number;
  created_at: string;
  updated_at: string;
  merged_at: string | null;
  web_url: string;
}

// ==================== SSE Event Types ====================

export interface SSEChunkEvent {
  type: 'chunk';
  content: string;
}

export interface SSEDoneEvent {
  type: 'done';
  content: string;
  title?: string;
}

export interface SSEErrorEvent {
  type: 'error';
  error: string;
  retCode?: string;
}

export type SSEEvent = SSEChunkEvent | SSEDoneEvent | SSEErrorEvent;
