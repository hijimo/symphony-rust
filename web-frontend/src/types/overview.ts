import type { KanbanTodoColumn, KanbanInProgressColumn, KanbanPrColumn } from './kanban';

export interface ProjectMeta {
  project_id: number;
  project_name: string;
  platform: 'gitlab' | 'github';
  namespace: string;
  repo_name: string;
}

export interface ProjectIssuesEntry extends ProjectMeta {
  todo: KanbanTodoColumn;
  in_progress: KanbanInProgressColumn;
  error: string | null;
}

export interface ProjectPrsEntry extends ProjectMeta {
  pr: KanbanPrColumn;
  error: string | null;
}

export interface OverviewIssuesResponse {
  projects: ProjectIssuesEntry[];
  total_running_projects: number;
  has_more: boolean;
}

export interface OverviewPrsResponse {
  projects: ProjectPrsEntry[];
  total_running_projects: number;
  has_more: boolean;
}

export interface OverviewParams {
  max_projects?: number;
  todo_limit?: number;
}
