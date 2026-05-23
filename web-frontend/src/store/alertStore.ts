import { create } from 'zustand';
import {
  getAlertHistory,
  getAlertRules,
  updateAlertRules,
  getAlertChannels,
  updateAlertChannels,
  testNotification,
} from '../api/alerts';
import type {
  AlertHistoryRecord,
  AlertHistoryQuery,
  AlertRule,
  UpdateAlertRulesRequest,
  UpdateAlertChannelsRequest,
  TestNotificationRequest,
  TestNotificationResponse,
  NotificationChannel,
  ChannelTypeInfo,
} from '../types/alert';
import type { PaginationData } from '../types';

interface AlertState {
  // Rules
  rules: AlertRule[];
  rulesLoading: boolean;
  rulesError: string | null;

  // Channels
  channels: NotificationChannel[];
  availableTypes: ChannelTypeInfo[];
  channelsLoading: boolean;
  channelsError: string | null;

  // History
  history: PaginationData<AlertHistoryRecord> | null;
  historyLoading: boolean;
  historyError: string | null;

  // Actions
  fetchRules: () => Promise<void>;
  fetchChannels: () => Promise<void>;
  fetchHistory: (query: AlertHistoryQuery) => Promise<void>;
  updateRules: (request: UpdateAlertRulesRequest) => Promise<void>;
  updateChannels: (request: UpdateAlertChannelsRequest) => Promise<void>;
  testNotification: (request: TestNotificationRequest) => Promise<TestNotificationResponse>;
  reset: () => void;
}

export const useAlertStore = create<AlertState>((set) => ({
  rules: [],
  rulesLoading: false,
  rulesError: null,

  channels: [],
  availableTypes: [],
  channelsLoading: false,
  channelsError: null,

  history: null,
  historyLoading: false,
  historyError: null,

  fetchRules: async () => {
    set({ rulesLoading: true, rulesError: null });
    try {
      const data = await getAlertRules();
      set({ rules: data.rules, rulesLoading: false });
    } catch (err) {
      set({
        rulesLoading: false,
        rulesError: err instanceof Error ? err.message : 'Failed to fetch alert rules',
      });
    }
  },

  fetchChannels: async () => {
    set({ channelsLoading: true, channelsError: null });
    try {
      const data = await getAlertChannels();
      set({
        channels: data.channels,
        availableTypes: data.availableTypes,
        channelsLoading: false,
      });
    } catch (err) {
      set({
        channelsLoading: false,
        channelsError: err instanceof Error ? err.message : 'Failed to fetch notification channels',
      });
    }
  },

  fetchHistory: async (query: AlertHistoryQuery) => {
    set({ historyLoading: true, historyError: null });
    try {
      const data = await getAlertHistory(query);
      set({ history: data, historyLoading: false });
    } catch (err) {
      set({
        historyLoading: false,
        historyError: err instanceof Error ? err.message : 'Failed to fetch alert history',
      });
    }
  },

  updateRules: async (request: UpdateAlertRulesRequest) => {
    try {
      const data = await updateAlertRules(request);
      set({ rules: data.rules });
    } catch (err) {
      throw err;
    }
  },

  updateChannels: async (request: UpdateAlertChannelsRequest) => {
    try {
      const data = await updateAlertChannels(request);
      set({ channels: data.channels });
    } catch (err) {
      throw err;
    }
  },

  testNotification: async (request: TestNotificationRequest) => {
    const result = await testNotification(request);
    return result;
  },

  reset: () => {
    set({
      rules: [],
      rulesLoading: false,
      rulesError: null,
      channels: [],
      availableTypes: [],
      channelsLoading: false,
      channelsError: null,
      history: null,
      historyLoading: false,
      historyError: null,
    });
  },
}));
