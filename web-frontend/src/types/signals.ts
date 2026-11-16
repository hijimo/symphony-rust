/** External signals (P3 §3). */
export type SignalSource = 'sentry' | 'alertmanager' | 'generic';

export interface SignalIngestKey {
  id: number;
  projectId: number;
  source: SignalSource;
  name: string;
  createdBy: number | null;
  createdAt: string;
  lastUsedAt: string | null;
  revokedAt: string | null;
}

export interface CreatedSignalKey extends SignalIngestKey {
  /** Shown once. */
  secret: string;
}
