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
