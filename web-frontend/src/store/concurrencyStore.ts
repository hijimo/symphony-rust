import { create } from 'zustand';
import {
  getGlobalConcurrency,
  getSseTicket,
  updateConcurrencyConfig,
  type ProjectConcurrencyInfo,
} from '../api/concurrency';

interface ConcurrencyState {
  globalMax: number;
  globalActive: number;
  utilizationPercent: number;
  projects: ProjectConcurrencyInfo[];
  dataFreshnessSeconds: number;
  loading: boolean;
  error: string | null;
  sseConnected: boolean;
  eventSource: EventSource | null;

  fetchStatus: () => Promise<void>;
  updateConfig: (params: { globalMax: number; expectedPrevious?: number }) => Promise<void>;
  connectSSE: () => Promise<void>;
  disconnectSSE: () => void;
  handleSSEEvent: (event: Record<string, unknown>) => void;
  handleSSEDisconnect: () => void;
  reset: () => void;
}

export const useConcurrencyStore = create<ConcurrencyState>((set, get) => ({
  globalMax: 0,
  globalActive: 0,
  utilizationPercent: 0,
  projects: [],
  dataFreshnessSeconds: 0,
  loading: false,
  error: null,
  sseConnected: false,
  eventSource: null,

  fetchStatus: async () => {
    set({ loading: true, error: null });
    try {
      const data = await getGlobalConcurrency();
      set({
        globalMax: data.global_max,
        globalActive: data.global_active,
        utilizationPercent: data.utilization_percent,
        projects: data.projects,
        dataFreshnessSeconds: data.data_freshness_seconds,
        loading: false,
      });
    } catch (err) {
      set({ loading: false, error: 'Failed to fetch concurrency status' });
    }
  },

  updateConfig: async ({ globalMax, expectedPrevious }) => {
    try {
      await updateConcurrencyConfig({
        global_max: globalMax,
        expected_previous: expectedPrevious,
      });
      set({ globalMax });
    } catch (err) {
      throw err;
    }
  },

  connectSSE: async () => {
    const { eventSource } = get();
    if (eventSource) return;

    try {
      const { ticket } = await getSseTicket();
      const baseUrl = import.meta.env.VITE_API_BASE_URL || '';
      const es = new EventSource(
        `${baseUrl}/api/admin/concurrency/events?ticket=${ticket}`
      );

      es.onopen = () => {
        set({ sseConnected: true });
      };

      es.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          get().handleSSEEvent(data);
        } catch {
          // ignore parse errors
        }
      };

      es.onerror = () => {
        get().handleSSEDisconnect();
      };

      set({ eventSource: es });
    } catch {
      set({ sseConnected: false });
    }
  },

  disconnectSSE: () => {
    const { eventSource } = get();
    if (eventSource) {
      eventSource.close();
      set({ eventSource: null, sseConnected: false });
    }
  },

  handleSSEEvent: (event) => {
    const type = event.type as string;
    if (
      type === 'agent_started' ||
      type === 'agent_completed' ||
      type === 'snapshot'
    ) {
      const globalActive = (event.global_active as number) ?? get().globalActive;
      const globalMax = (event.global_max as number) ?? get().globalMax;
      const utilization = globalMax > 0 ? (globalActive / globalMax) * 100 : 0;

      set({
        globalActive,
        globalMax,
        utilizationPercent: utilization,
      });

      if (type === 'snapshot' && Array.isArray(event.projects)) {
        set({ projects: event.projects as ProjectConcurrencyInfo[] });
      }
    }
  },

  handleSSEDisconnect: () => {
    const { eventSource } = get();
    if (eventSource) {
      eventSource.close();
    }
    set({ eventSource: null, sseConnected: false });

    // Reconnect after 3 seconds
    setTimeout(() => {
      get().connectSSE();
    }, 3000);
  },

  reset: () => {
    const { eventSource } = get();
    if (eventSource) eventSource.close();
    set({
      globalMax: 0,
      globalActive: 0,
      utilizationPercent: 0,
      projects: [],
      dataFreshnessSeconds: 0,
      loading: false,
      error: null,
      sseConnected: false,
      eventSource: null,
    });
  },
}));
