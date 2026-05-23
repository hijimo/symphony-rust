// Phase 5 - Alert & Notification types

export type Severity = 'critical' | 'warning' | 'info';
export type NotificationStatus = 'sent' | 'failed' | 'suppressed' | 'pending';

export interface AlertRule {
  ruleId: string;
  name: string;
  description: string;
  severity: Severity;
  enabled: boolean;
  threshold: Record<string, number>;
  cooldownSeconds: number;
  updatedAt: string;
}

export interface AlertHistoryRecord {
  id: number;
  ruleId: string;
  severity: Severity;
  projectId: number | null;
  projectName: string | null;
  title: string;
  message: string;
  context: Record<string, string> | null;
  firedAt: string;
  resolvedAt: string | null;
  notifiedAt: string | null;
  notificationChannel: string | null;
  notificationStatus: NotificationStatus | null;
}

export interface NotificationChannel {
  channelId: string;
  name: string;
  channelType: string;
  enabled: boolean;
  config: Record<string, unknown>;
  configMasked: boolean;
  severityFilter: Severity[];
  lastTestAt: string | null;
  lastTestSuccess: boolean | null;
  updatedAt: string;
}

export interface ChannelTypeInfo {
  type: string;
  name: string;
  configSchema: Record<string, ConfigFieldSchema>;
  status?: string;
}

export interface ConfigFieldSchema {
  type: string;
  required: boolean;
  description: string;
}

// --- Request types ---

export interface UpdateAlertRulesRequest {
  rules: Array<{
    ruleId: string;
    enabled?: boolean;
    threshold?: Record<string, number>;
    cooldownSeconds?: number;
  }>;
}

export interface UpdateAlertChannelsRequest {
  channels: Array<{
    channelId?: string;
    name: string;
    channelType: string;
    enabled: boolean;
    config: Record<string, unknown>;
    severityFilter: Severity[];
  }>;
}

export interface TestNotificationRequest {
  channelId: string;
  message?: string;
}

// --- Response types ---

export interface AlertRulesResponse {
  rules: AlertRule[];
}

export interface UpdateAlertRulesResponse {
  updatedCount: number;
  rules: AlertRule[];
}

export interface AlertChannelsResponse {
  channels: NotificationChannel[];
  availableTypes: ChannelTypeInfo[];
}

export interface UpdateAlertChannelsResponse {
  channels: NotificationChannel[];
}

export interface TestNotificationResponse {
  success: boolean;
  channelId: string;
  channelType: string;
  sentAt: string;
  responseTimeMs: number;
}

// --- Query types ---

export interface AlertHistoryQuery {
  page_no?: number;
  page_size?: number;
  severity?: Severity;
  rule_id?: string;
  project_id?: number;
  status?: NotificationStatus;
  start_time?: string;
  end_time?: string;
}
