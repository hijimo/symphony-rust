// AI-native SDLC document flow + workbench types (camelCase from backend).

export type DocType = 'intent' | 'spec' | 'plan';
export type DocStatus =
  | 'draft'
  | 'in_review'
  | 'changes_requested'
  | 'approved'
  | 'stale'
  | 'rejected'
  | 'merged';
export type Track = 'full' | 'plan';
export type FlowState =
  | 'intent'
  | 'intent_review'
  | 'spec'
  | 'spec_review'
  | 'plan'
  | 'plan_review'
  | 'plan_approved_pending_merge'
  | 'todo'
  | 'rejected';
export type GateAction = 'approve' | 'request_changes' | 'reject';

export interface ChangeFlow {
  id: number;
  projectId: number;
  issueIid: number;
  track: Track;
  revision: number;
  branch: string;
  docsMrIid: number | null;
  docsMrUrl: string | null;
  mergeCommitSha: string | null;
  mergeError: string | null;
  baseSha: string | null;
  closed: boolean;
  createdBy: number;
  createdAt: string;
  updatedAt: string;
}

export interface ChangeDocument {
  id: number;
  flowId: number;
  docType: DocType;
  status: DocStatus;
  path: string;
  currentBlobSha: string | null;
  templateRef: string | null;
  draftState: 'idle' | 'running' | 'failed';
  draftFailures: number;
  draftError: string | null;
  lastFeedback: string | null;
  updatedAt: string;
}

export interface DocumentApproval {
  id: number;
  documentId: number;
  gate: 'G1' | 'G2' | 'G3';
  action: GateAction;
  actor: number;
  actorName: string | null;
  comment: string | null;
  blobSha: string;
  commitSha: string | null;
  createdAt: string;
  invalidatedAt: string | null;
}

export interface FlowView {
  flow: ChangeFlow;
  state: FlowState;
  label: string | null;
  documents: ChangeDocument[];
  approvals: DocumentApproval[];
  writableDir: string;
  multiRepo: boolean;
  primaryDir: string;
  runningStage: DocType | null;
  mergePrecheck?: { canMerge: boolean; reason: string | null };
}

export interface TreeEntry {
  name: string;
  path: string;
  kind: 'dir' | 'file';
  size: number | null;
  writable: boolean;
}

export interface FileContent {
  path: string;
  content: string | null;
  version: string;
  size: number;
  binary: boolean;
  tooLarge: boolean;
  writable: boolean;
}

export interface SearchHit {
  path: string;
  line: number | null;
  text: string | null;
}

export interface ContextStatus {
  baseSha: string | null;
  behindMain: boolean;
  missing: string[];
}

export interface DocTemplate {
  docType: DocType;
  source: 'builtin' | 'project';
  templateRef: string;
  skeleton: string;
  prompt: string;
  checklist: string[];
}

export interface ValidationIssue {
  section: string;
  message: string;
}

// ------------------------------------------------------------ stage kanban

export type StageKey = 'plan' | 'design' | 'build' | 'deploy' | 'maintain';

export interface StagePlacement {
  column: StageKey;
  subState: string;
  anomaly: boolean;
  waitingHuman: boolean;
}

export interface StageIssueCard {
  iid: number;
  title: string;
  state: string;
  labels: string[];
  author: { username: string; display_name: string | null; avatar_url: string | null };
  updatedAt: string;
  webUrl: string;
  placement: StagePlacement;
  track: string | null;
  flow: {
    state: FlowState;
    track: Track;
    docsMrUrl: string | null;
    mergeError: string | null;
    documents: { docType: DocType; status: DocStatus; draftState: string }[];
  } | null;
}

export interface StageColumnData {
  key: StageKey;
  title: string;
  hint: string;
  totalCount: number;
  issues: StageIssueCard[];
  mergeRequests: {
    iid: number;
    title: string;
    web_url: string;
    source_branch: string;
    ci_status: string | null;
    author: { username: string };
  }[];
}

export interface StageKanban {
  columns: StageColumnData[];
  prError: string | null;
  platform: string;
}

// ---------------------------------------------------------- agent events

export type AgentEvent =
  | { type: 'user_message'; text: string; mode: string }
  | { type: 'turn_started'; turnId: string | null }
  | { type: 'message_delta'; itemId: string; delta: string }
  | { type: 'reasoning_delta'; itemId: string; delta: string }
  | { type: 'command_output'; itemId: string; delta: string }
  | { type: 'diff'; diff: string }
  | { type: 'turn_diff'; diff: string }
  | {
      type: 'item';
      phase: 'started' | 'completed';
      item: {
        id: string;
        kind: string;
        text?: string;
        command?: string;
        status?: string;
        exitCode?: number | null;
        output?: string | null;
        paths?: string[];
        summary?: string[];
      };
    }
  | {
      type: 'approval_request';
      requestId: string;
      kind: string;
      command: string | null;
      cwd: string | null;
      reason: string | null;
    }
  | {
      type: 'user_input_request';
      requestId: string;
      questions: { id: string; header: string; question: string; options?: { label: string; description: string }[] }[];
    }
  | { type: 'request_resolved'; requestId: string }
  | { type: 'notice'; level: 'info' | 'warning' | 'error'; message: string }
  | { type: 'error'; message: string }
  | { type: 'turn_completed'; status: string; error: string | null }
  | { type: 'lagged' };
