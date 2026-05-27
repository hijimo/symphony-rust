import { create } from 'zustand';
import type { ProjectIssuesEntry, ProjectPrsEntry, ProjectMeta } from '../types/overview';
import { getOverviewIssues, getOverviewPrs } from '../api/overview';

interface OverviewKanbanState {
  projectIssues: Map<number, ProjectIssuesEntry>;
  projectPrs: Map<number, ProjectPrsEntry>;
  projectMetas: ProjectMeta[];

  issuesLoading: boolean;
  prsLoading: boolean;
  issuesError: string | null;
  prsError: string | null;

  totalRunningProjects: number;
  hasMore: boolean;

  fetchIssues: (signal?: AbortSignal) => Promise<void>;
  fetchPrs: (signal?: AbortSignal) => Promise<void>;
  reset: () => void;
}

export const useOverviewKanbanStore = create<OverviewKanbanState>((set) => ({
  projectIssues: new Map(),
  projectPrs: new Map(),
  projectMetas: [],

  issuesLoading: false,
  prsLoading: false,
  issuesError: null,
  prsError: null,

  totalRunningProjects: 0,
  hasMore: false,

  fetchIssues: async (signal?: AbortSignal) => {
    set({ issuesLoading: true, issuesError: null });
    try {
      const data = await getOverviewIssues({ max_projects: 8, todo_limit: 5 }, signal);
      const issuesMap = new Map<number, ProjectIssuesEntry>();
      const metas: ProjectMeta[] = [];

      for (const project of data.projects) {
        issuesMap.set(project.project_id, project);
        metas.push({
          project_id: project.project_id,
          project_name: project.project_name,
          platform: project.platform,
          namespace: project.namespace,
          repo_name: project.repo_name,
        });
      }

      set({
        projectIssues: issuesMap,
        projectMetas: metas,
        totalRunningProjects: data.total_running_projects,
        hasMore: data.has_more,
        issuesLoading: false,
      });
    } catch (err: any) {
      if (err?.name === 'CanceledError' || err?.code === 'ERR_CANCELED') {
        set({ issuesLoading: false });
        return;
      }
      set({ issuesError: err?.message || '获取总览数据失败', issuesLoading: false });
    }
  },

  fetchPrs: async (signal?: AbortSignal) => {
    set({ prsLoading: true, prsError: null });
    try {
      const data = await getOverviewPrs({ max_projects: 8 }, signal);
      const prsMap = new Map<number, ProjectPrsEntry>();

      for (const project of data.projects) {
        prsMap.set(project.project_id, project);
      }

      set({ projectPrs: prsMap, prsLoading: false });
    } catch (err: any) {
      if (err?.name === 'CanceledError' || err?.code === 'ERR_CANCELED') {
        set({ prsLoading: false });
        return;
      }
      set({ prsError: err?.message || '获取 PR 数据失败', prsLoading: false });
    }
  },

  reset: () => {
    set({
      projectIssues: new Map(),
      projectPrs: new Map(),
      projectMetas: [],
      issuesLoading: false,
      prsLoading: false,
      issuesError: null,
      prsError: null,
      totalRunningProjects: 0,
      hasMore: false,
    });
  },
}));
