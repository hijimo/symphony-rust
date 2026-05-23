// Phase 4 - Contributors types

export interface Contributor {
  username: string;
  displayName: string | null;
  avatarUrl: string | null;
  recentIssueCount: number;
  recentMrCount: number;
  lastActivityAt: string;
  isBot: boolean;
}

export interface ContributorsResponse {
  contributors: Contributor[];
  totalCount: number;
  scope: string;
  cached: boolean;
  cachedAt: string | null;
}
