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

export type SignalSeverity = 'critical' | 'warning' | 'info';

/** A stored signal (`GET /projects/{id}/signals`). */
export interface SignalRecord {
  id: number;
  projectId: number;
  /** Set on the global board. */
  projectName?: string;
  source: SignalSource;
  fingerprint: string;
  title: string;
  severity: SignalSeverity;
  status: 'new' | 'intent' | 'ignored';
  resolved: boolean;
  firstSeenAt: string;
  lastSeenAt: string;
  count: number;
  evidence: { message?: string | null; details?: unknown };
  externalUrl: string | null;
  flowId: number | null;
  issueIid: number | null;
  ignoredUntil: string | null;
  triagedBy: number | null;
  triagedAt: string | null;
}
