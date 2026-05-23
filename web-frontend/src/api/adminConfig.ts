import client from './client';

export interface SystemConfigItem {
  key: string;
  value: string;
  description: string | null;
  updatedAt: string;
}

export interface ConfigEntry {
  key: string;
  value: string;
}

export interface UpdateConfigRequest {
  configs: ConfigEntry[];
}

export interface SystemStats {
  totalProjects: number;
  runningServices: number;
  totalUsers: number;
  globalConcurrencyLimit: number;
  globalConcurrencyUsed: number;
}

export async function getSystemConfig(): Promise<SystemConfigItem[]> {
  const resp = await client.get('/admin/config');
  return resp.data.data;
}

export async function updateSystemConfig(
  request: UpdateConfigRequest
): Promise<SystemConfigItem[]> {
  const resp = await client.put('/admin/config', request);
  return resp.data.data;
}

export async function getSystemStats(): Promise<SystemStats> {
  const resp = await client.get('/admin/stats');
  return resp.data.data;
}
