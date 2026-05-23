import client from './client';

export type ProxyMode = 'disabled' | 'inherit_env' | 'manual';
export type SecretAction = 'keep' | 'set' | 'clear';

export interface ProxySecretDisplay {
  configured: boolean;
  displayValue: string;
  updatedAt: string | null;
}

export interface ProxyWarning {
  code: string;
  severity: 'info' | 'warning' | 'error';
  blocking: boolean;
  message: string;
}

export interface NetworkProxyConfig {
  mode: ProxyMode;
  version: string;
  source: string;
  httpProxy: ProxySecretDisplay;
  httpsProxy: ProxySecretDisplay;
  allProxy: ProxySecretDisplay;
  noProxy: string;
  autoBypassLocal: boolean;
  needsRestartProjectCount: number;
  updatedAt: string | null;
  warnings: ProxyWarning[];
}

export interface SecretUpdate {
  action: SecretAction;
  value?: string;
}

export interface UpdateNetworkProxyRequest {
  expectedVersion: string;
  mode: ProxyMode;
  httpProxy: SecretUpdate;
  httpsProxy: SecretUpdate;
  allProxy: SecretUpdate;
  noProxy: string;
  autoBypassLocal: boolean;
}

export interface TestNetworkProxyRequest {
  targetId: string;
  useDraftConfig: boolean;
  draftConfig?: UpdateNetworkProxyRequest;
}

export interface ProxyTestResult {
  status: string;
  targetHost: string;
  proxyUsed: boolean;
  proxySummary: string;
  durationMs: number;
  message: string;
}

export async function getNetworkProxy(): Promise<NetworkProxyConfig> {
  const resp = await client.get('/admin/network-proxy');
  return resp.data.data;
}

export async function updateNetworkProxy(
  request: UpdateNetworkProxyRequest,
): Promise<NetworkProxyConfig> {
  const resp = await client.put('/admin/network-proxy', request);
  return resp.data.data;
}

export async function testNetworkProxy(targetId: string): Promise<ProxyTestResult> {
  const resp = await client.post('/admin/network-proxy/test', { targetId, useDraftConfig: false });
  return resp.data.data;
}

export async function testNetworkProxyDraft(request: TestNetworkProxyRequest): Promise<ProxyTestResult> {
  const resp = await client.post('/admin/network-proxy/test', request);
  return resp.data.data;
}
