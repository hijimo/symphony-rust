import client from './client';
import type { ResponseData } from '../types';
import type { CreatedSignalKey, SignalIngestKey, SignalSource } from '../types/signals';

export async function listSignalKeys(projectId: number): Promise<SignalIngestKey[]> {
  const res = await client.get<ResponseData<SignalIngestKey[]>>(`/projects/${projectId}/signal-keys`);
  return res.data.data;
}

export async function createSignalKey(
  projectId: number,
  body: { source: SignalSource; name: string },
): Promise<CreatedSignalKey> {
  const res = await client.post<ResponseData<CreatedSignalKey>>(`/projects/${projectId}/signal-keys`, body);
  return res.data.data;
}

export async function revokeSignalKey(projectId: number, keyId: number): Promise<void> {
  await client.delete(`/projects/${projectId}/signal-keys/${keyId}`);
}

/** `days`: 1 or 7; null ignores permanently. */
export async function ignoreSignal(projectId: number, signalId: number, days: 1 | 7 | null): Promise<void> {
  await client.post(`/projects/${projectId}/signals/${signalId}/ignore`, { days });
}

/** Start an incident document flow from the signal (P3 §3 S4). */
export async function createSignalIntent(projectId: number, signalId: number): Promise<{ flow: { issueIid: number } }> {
  const res = await client.post<ResponseData<{ flow: { issueIid: number } }>>(
    `/projects/${projectId}/signals/${signalId}/intent`,
    {},
    // Creates the issue, branch, workspace and docs MR.
    { timeout: 120_000 },
  );
  return res.data.data;
}
