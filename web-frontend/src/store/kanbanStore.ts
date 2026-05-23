import { create } from 'zustand';
import type { KanbanData, KanbanParams } from '../types/kanban';
import { getKanban } from '../api/kanban';

interface KanbanState {
  kanbanData: KanbanData | null;
  loading: boolean;
  error: string | null;
  filters: KanbanParams;
  fetchKanban: (projectId: number) => Promise<void>;
  refresh: (projectId: number) => Promise<void>;
  setFilters: (filters: Partial<KanbanParams>) => void;
  clearError: () => void;
}

export const useKanbanStore = create<KanbanState>((set, get) => ({
  kanbanData: null,
  loading: false,
  error: null,
  filters: {},

  fetchKanban: async (projectId: number) => {
    set({ loading: true, error: null });
    try {
      const data = await getKanban(projectId, get().filters);
      set({ kanbanData: data });
    } catch (err: any) {
      set({ error: err?.message || '获取看板数据失败' });
    } finally {
      set({ loading: false });
    }
  },

  refresh: async (projectId: number) => {
    set({ loading: true, error: null });
    try {
      const data = await getKanban(projectId, get().filters, { noCache: true });
      set({ kanbanData: data });
    } catch (err: any) {
      set({ error: err?.message || '刷新看板数据失败' });
    } finally {
      set({ loading: false });
    }
  },

  setFilters: (filters: Partial<KanbanParams>) => {
    set((state) => ({
      filters: { ...state.filters, ...filters },
    }));
  },

  clearError: () => {
    set({ error: null });
  },
}));
