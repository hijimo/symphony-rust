/** Organization rules repository (P3 design §4 O1/O4). */
export interface OrgConfigPin {
  projectId: number;
  pin: string;
  sha: string | null;
  error: string | null;
}

export interface OrgConfigStatus {
  enabled: boolean;
  url: string | null;
  ref: string;
  intervalSecs: number;
  currentSha: string | null;
  lastSyncAt: string | null;
  lastError: string | null;
  pins: OrgConfigPin[];
  versions: string[];
  /** `config.toml` entries left out of CODEX_HOME. */
  droppedConfig: string[];
}

export interface ProjectOrgConfig {
  enabled: boolean;
  url: string | null;
  ref: string;
  followedSha: string | null;
  pin: string | null;
  effectiveSha: string | null;
  pinError: string | null;
  canEdit: boolean;
}
