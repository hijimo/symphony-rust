import { create } from 'zustand';
import type { KanbanData, KanbanIssuesData, KanbanParams, KanbanPrsData } from '../types/kanban';
import { getKanbanIssues, getKanbanPrs } from '../api/kanban';

interface KanbanState {
  kanbanData: KanbanData | null;
  loading: boolean;
  prsLoading: boolean;
  error: string | null;
  prsError: string | null;
  filters: KanbanParams;
  fetchKanban: (projectId: number) => Promise<void>;
  refresh: (projectId: number) => Promise<void>;
  setFilters: (filters: Partial<KanbanParams>) => void;
  clearError: () => void;
  reset: () => void;
}

function assembleKanbanData(
  issues: KanbanIssuesData | null,
  prs: KanbanPrsData | null,
): KanbanData | null {
  if (!issues) return null;
  return {
    todo: issues.todo,
    in_progress: issues.in_progress,
    testing: issues.testing,
    pr: prs?.pr ?? { merge_requests: [], total_count: 0 },
    cached: issues.cached || (prs?.cached ?? false),
    cached_at: issues.cached_at ?? prs?.cached_at ?? null,
    platform: issues.platform,
  };
}

export const useKanbanStore = create<KanbanState>((set, get) => ({
  kanbanData: null,
  loading: false,
  prsLoading: false,
  error: null,
  prsError: null,
  filters: {},

  fetchKanban: async (projectId: number) => {
    set({ loading: true, prsLoading: true, error: null, prsError: null });

    const filters = get().filters;
    let issuesData: KanbanIssuesData | null = null;
    let prsData: KanbanPrsData | null = null;

    const issuesPromise = getKanbanIssues(projectId, filters)
      .then((data) => {
        issuesData = data;
        set({ kanbanData: assembleKanbanData(data, prsData), loading: false });
      })
      .catch((err: any) => {
        set({ error: err?.message || '获取看板数据失败', loading: false });
      });

    const prsPromise = getKanbanPrs(projectId)
      .then((data) => {
        prsData = data;
        set({ kanbanData: assembleKanbanData(issuesData, data), prsLoading: false });
      })
      .catch((err: any) => {
        set({ prsError: err?.message || '获取 PR 数据失败', prsLoading: false });
      });

    await Promise.allSettled([issuesPromise, prsPromise]);
  },

  refresh: async (projectId: number) => {
    set({ loading: true, prsLoading: true, error: null, prsError: null });

    const filters = get().filters;
    let issuesData: KanbanIssuesData | null = null;
    let prsData: KanbanPrsData | null = null;

    const issuesPromise = getKanbanIssues(projectId, filters, { noCache: true })
      .then((data) => {
        issuesData = data;
        set({ kanbanData: assembleKanbanData(data, prsData), loading: false });
      })
      .catch((err: any) => {
        set({ error: err?.message || '刷新看板数据失败', loading: false });
      });

    const prsPromise = getKanbanPrs(projectId, { noCache: true })
      .then((data) => {
        prsData = data;
        set({ kanbanData: assembleKanbanData(issuesData, data), prsLoading: false });
      })
      .catch((err: any) => {
        set({ prsError: err?.message || '刷新 PR 数据失败', prsLoading: false });
      });

    await Promise.allSettled([issuesPromise, prsPromise]);
  },

  setFilters: (filters: Partial<KanbanParams>) => {
    set((state) => ({
      filters: { ...state.filters, ...filters },
    }));
  },

  clearError: () => {
    set({ error: null, prsError: null });
  },

  reset: () => {
    set({
      kanbanData: null,
      loading: false,
      prsLoading: false,
      error: null,
      prsError: null,
      filters: {},
    });
  },
}));
