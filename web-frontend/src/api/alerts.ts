import client from './client';
import type { PaginationData } from '../types';
import type {
  AlertHistoryRecord,
  AlertHistoryQuery,
  AlertRule,
  AlertRulesResponse,
  UpdateAlertRulesRequest,
  UpdateAlertRulesResponse,
  AlertChannelsResponse,
  UpdateAlertChannelsRequest,
  UpdateAlertChannelsResponse,
  TestNotificationRequest,
  TestNotificationResponse,
} from '../types/alert';

export type {
  AlertHistoryRecord,
  AlertHistoryQuery,
  AlertRule,
  AlertRulesResponse,
  UpdateAlertRulesResponse,
  AlertChannelsResponse,
  UpdateAlertChannelsResponse,
  TestNotificationResponse,
};

export async function getAlertHistory(
  query: AlertHistoryQuery
): Promise<PaginationData<AlertHistoryRecord>> {
  const resp = await client.get('/admin/alerts', { params: query });
  return resp.data.data;
}

export async function getAlertRules(): Promise<AlertRulesResponse> {
  const resp = await client.get('/admin/alerts/rules');
  return resp.data.data;
}

export async function updateAlertRules(
  request: UpdateAlertRulesRequest
): Promise<UpdateAlertRulesResponse> {
  const resp = await client.put('/admin/alerts/rules', request);
  return resp.data.data;
}

export async function getAlertChannels(): Promise<AlertChannelsResponse> {
  const resp = await client.get('/admin/alerts/channels');
  return resp.data.data;
}

export async function updateAlertChannels(
  request: UpdateAlertChannelsRequest
): Promise<UpdateAlertChannelsResponse> {
  const resp = await client.put('/admin/alerts/channels', request);
  return resp.data.data;
}

export async function testNotification(
  request: TestNotificationRequest
): Promise<TestNotificationResponse> {
  const resp = await client.post('/admin/alerts/test', request);
  return resp.data.data;
}
