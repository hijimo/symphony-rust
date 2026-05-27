// Phase 3 - Kanban types
// Entity fields use snake_case (data comes from GitLab/GitHub API)

export interface PlatformUser {
  username: string;
  display_name: string | null;
  avatar_url: string | null;
}

export interface KanbanIssue {
  iid: number;
  title: string;
  state: 'opened' | 'closed';
  labels: string[];
  author: PlatformUser;
  assignees: PlatformUser[];
  created_at: string;
  updated_at: string;
  web_url: string;
  mr_count: number | null;
}

export type CiStatus = 'pending' | 'running' | 'success' | 'failed' | 'canceled';
export type ReviewStatus = 'pending' | 'approved' | 'changes_requested';

export interface KanbanMergeRequest {
  iid: number;
  title: string;
  state: 'opened' | 'closed' | 'merged';
  repository: string;
  author: PlatformUser;
  source_branch: string;
  target_branch: string;
  ci_status: CiStatus | null;
  review_status: ReviewStatus | null;
  related_issue_iids: number[];
  created_at: string;
  updated_at: string;
  web_url: string;
}

export interface KanbanTodoColumn {
  issues: KanbanIssue[];
  total_count: number;
  has_more: boolean;
}

export interface KanbanInProgressColumn {
  issues: KanbanIssue[];
  total_count: number;
}

export interface KanbanPrColumn {
  merge_requests: KanbanMergeRequest[];
  total_count: number;
  error?: string | null;
}

export interface KanbanData {
  todo: KanbanTodoColumn;
  in_progress: KanbanInProgressColumn;
  pr: KanbanPrColumn;
  cached: boolean;
  cached_at: string | null;
  platform?: 'gitlab' | 'github';
}

export interface KanbanParams {
  todo_limit?: number;
  assignee?: string;
  author?: string;
  labels?: string;
  search?: string;
  no_cache?: boolean;
}

// Split endpoint response types

export interface KanbanIssuesData {
  todo: KanbanTodoColumn;
  in_progress: KanbanInProgressColumn;
  platform: 'gitlab' | 'github';
  cached: boolean;
  cached_at: string | null;
}

export interface KanbanPrsData {
  pr: KanbanPrColumn;
  platform: 'gitlab' | 'github';
  cached: boolean;
  cached_at: string | null;
}
