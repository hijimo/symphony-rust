export type UserRole = 'admin' | 'user';

export interface ResponseData<T = unknown> {
  data: T;
  success: boolean;
  retCode: string;
  retMsg: string;
  showType?: number;
}

export interface PaginationData<T = unknown> {
  limit: number;
  offset: number;
  pageNo: number;
  pageSize: number;
  pages: number;
  records: T[];
  totalCount: number;
}

export interface UserInfo {
  id: number;
  username: string;
  displayName: string | null;
  role: UserRole;
}

export interface UserProfile extends UserInfo {
  createdAt: string;
  updatedAt: string;
}

export interface UserConfig {
  hasGitlabToken: boolean;
  gitlabHost: string | null;
  hasGithubToken: boolean;
}

export interface CreateUserParams {
  username: string;
  password: string;
  displayName?: string;
  role: UserRole;
}

export interface ResetPasswordParams {
  newPassword: string;
}

export interface UpdateProfileParams {
  displayName: string;
}

export interface UpdateConfigParams {
  gitlabToken?: string;
  gitlabHost?: string;
  githubToken?: string;
}

export interface ChangePasswordParams {
  oldPassword: string;
  newPassword: string;
}

export interface GetUsersParams {
  pageNo: number;
  pageSize: number;
  search?: string;
  role?: string;
}

// Phase 2 - Project types

export type ProjectPlatform = 'gitlab' | 'github';
export type ServiceStatus = 'running' | 'stopped' | 'starting' | 'stopping' | 'error' | 'failed';
export type ProjectMemberRole = 'owner' | 'member';
export type WorkflowTemplateMode = 'default' | 'custom';

export interface Project {
  id: number;
  name: string;
  description: string | null;
  git_url: string;
  platform: ProjectPlatform;
  platform_host: string | null;
  namespace: string;
  repo_name: string;
  default_branch: string;
  workflow_template: WorkflowTemplateMode;
  service_status: ServiceStatus;
  service_pid: number | null;
  max_concurrent_agents: number;
  auto_restart: boolean;
  member_count: number;
  my_role: 'owner' | 'member' | 'admin' | null;
  created_by: number;
  created_at: string;
  updated_at: string;
  hooks_after_create: string | null;
  hooks_before_remove: string | null;
  codex_command: string | null;
  codex_approval_policy: string | null;
  codex_sandbox: string | null;
  testing_enabled: boolean;
  testing_max_attempts: number;
  testing_max_turns: number;
  testing_skip_labels: string | null;
  testing_allowed_commands: string | null;
}

export interface ProjectMember {
  user_id: number;
  username: string;
  display_name: string | null;
  role: ProjectMemberRole;
  synced_from: ProjectPlatform | null;
  created_at: string;
}

export interface ServiceStatusData {
  status: ServiceStatus;
  pid: number | null;
  started_at: string | null;
  uptime_seconds: number | null;
  restart_count: number;
  error_message: string | null;
  testing_status?: string | null;
  testing_pid?: number | null;
}

export interface WorkflowData {
  template_mode: WorkflowTemplateMode;
  content: string;
  updated_at: string;
}

export interface SyncResult {
  added: number;
  skipped: number;
  unmatched: string[];
}

export interface UpdateProjectParams {
  name?: string;
  description?: string;
  default_branch?: string;
  max_concurrent_agents?: number;
  auto_restart?: boolean;
  hooks_after_create?: string;
  hooks_before_remove?: string;
  codex_command?: string;
  codex_approval_policy?: string;
  codex_sandbox?: string;
  testing_enabled?: boolean;
  testing_max_attempts?: number;
  testing_max_turns?: number;
  testing_skip_labels?: string;
  testing_allowed_commands?: string;
}

export interface AddMemberParams {
  user_id: number;
  role?: ProjectMemberRole;
}

export interface UpdateMemberRoleParams {
  role: ProjectMemberRole;
}

export interface UpdateWorkflowParams {
  template_mode: WorkflowTemplateMode;
  content?: string;
}
