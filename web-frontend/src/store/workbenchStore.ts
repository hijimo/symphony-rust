import { create } from 'zustand';
import * as api from '../api/changes';
import type {
  AgentEvent,
  ContextStatus,
  DocType,
  FileContent,
  FlowView,
  TreeEntry,
} from '../types/changes';

// ---------------------------------------------------------------- types

export interface Buffer {
  path: string;
  content: string;
  /** Version the buffer was loaded from (optimistic lock). */
  baseVersion: string | null;
  dirty: boolean;
  writable: boolean;
  binary: boolean;
  tooLarge: boolean;
  /** Disk content changed while the buffer had local edits. */
  conflict: { content: string; version: string } | null;
}

export interface ChatEntry {
  id: string;
  role: 'user' | 'assistant' | 'reasoning' | 'command' | 'file_change' | 'notice' | 'approval' | 'question' | 'diff' | 'status';
  text: string;
  meta?: Record<string, unknown>;
}

export interface PendingRequest {
  requestId: string;
  kind: 'approval' | 'question';
  event: AgentEvent;
}

export interface TurnDiff {
  diff: string;
  baseSha: string | null;
  files: string[];
}

interface StageState {
  chat: ChatEntry[];
  running: boolean;
  pending: PendingRequest[];
  lastDiff: TurnDiff | null;
}

const emptyStage = (): StageState => ({ chat: [], running: false, pending: [], lastDiff: null });

interface WorkbenchState {
  projectId: number;
  iid: number;
  flow: FlowView | null;
  context: ContextStatus | null;
  loading: boolean;
  error: string | null;
  tree: Record<string, TreeEntry[]>;
  expanded: Record<string, boolean>;
  buffers: Record<string, Buffer>;
  tabs: string[];
  active: string | null;
  stage: DocType;
  stages: Record<DocType, StageState>;
  connected: boolean;
  notices: { id: number; level: string; message: string }[];
  /** Paths changed by the last AI turn (tree dot markers). */
  aiTouched: Record<string, boolean>;
  /** Pending "jump to line" request for the editor. */
  revealLine: { path: string; line: number } | null;

  init: (projectId: number, iid: number) => Promise<void>;
  dispose: () => void;
  refreshFlow: () => Promise<void>;
  loadDir: (path: string) => Promise<void>;
  toggleDir: (path: string) => Promise<void>;
  openFile: (path: string) => Promise<void>;
  closeTab: (path: string) => void;
  setActive: (path: string) => void;
  edit: (path: string, content: string) => void;
  save: (path: string, commit: boolean) => Promise<void>;
  reloadFromDisk: (path: string) => Promise<void>;
  resolveConflict: (path: string, keep: 'mine' | 'theirs') => void;
  setStage: (stage: DocType) => void;
  send: (text: string, extra?: { files?: string[]; selection?: SelectionRef }) => boolean;
  steer: (text: string) => void;
  interrupt: () => void;
  answer: (requestId: string, answer: unknown) => void;
  dismissNotice: (id: number) => void;
  pushNotice: (level: string, message: string) => void;
  applyFlow: (flow: FlowView) => void;
}

export interface SelectionRef {
  path: string;
  startLine: number;
  endLine: number;
  text: string;
}

let socket: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
/** Bumped by init/dispose; stale connects and reconnects are dropped (verify B I3). */
let sessionGen = 0;
/** In-flight save per path, so explicit saves wait for autosaves (verify B I7). */
const inflight: Record<string, Promise<void>> = {};
let autosaveTimers: Record<string, ReturnType<typeof setTimeout>> = {};
let noticeSeq = 0;
let entrySeq = 0;
const AUTOSAVE_MS = 1500;

const nextId = () => `e${++entrySeq}`;

/** Reduce an agent event into the stage chat. Pure; exported for tests. */
export function reduceAgentEvent(st: StageState, ev: AgentEvent): StageState {
  const chat = [...st.chat];
  const upsertDelta = (role: ChatEntry['role'], itemId: string, delta: string) => {
    const idx = chat.findIndex((c) => c.role === role && c.meta?.itemId === itemId);
    if (idx >= 0) chat[idx] = { ...chat[idx], text: chat[idx].text + delta };
    else chat.push({ id: nextId(), role, text: delta, meta: { itemId } });
  };
  switch (ev.type) {
    case 'user_message':
      chat.push({ id: nextId(), role: 'user', text: ev.text, meta: { mode: ev.mode } });
      return { ...st, chat, running: true };
    case 'turn_started':
      return { ...st, running: true };
    case 'message_delta':
      upsertDelta('assistant', ev.itemId, ev.delta);
      return { ...st, chat };
    case 'reasoning_delta':
      upsertDelta('reasoning', ev.itemId, ev.delta);
      return { ...st, chat };
    case 'command_output': {
      const idx = chat.findIndex((c) => c.role === 'command' && c.meta?.itemId === ev.itemId);
      if (idx >= 0) {
        const out = String(chat[idx].meta?.output ?? '') + ev.delta;
        chat[idx] = { ...chat[idx], meta: { ...chat[idx].meta, output: out } };
      }
      return { ...st, chat };
    }
    case 'item': {
      const it = ev.item;
      if (it.kind === 'message') {
        const idx = chat.findIndex((c) => c.role === 'assistant' && c.meta?.itemId === it.id);
        const text = it.text ?? '';
        if (idx >= 0) chat[idx] = { ...chat[idx], text: text || chat[idx].text };
        else if (text) chat.push({ id: nextId(), role: 'assistant', text, meta: { itemId: it.id } });
        return { ...st, chat };
      }
      if (it.kind === 'command' || it.kind === 'file_change') {
        const role = it.kind === 'command' ? 'command' : 'file_change';
        const idx = chat.findIndex((c) => c.role === role && c.meta?.itemId === it.id);
        const text = it.kind === 'command' ? (it.command ?? '') : (it.paths ?? []).join(', ');
        const meta = { ...(idx >= 0 ? chat[idx].meta : {}), itemId: it.id, status: it.status, exitCode: it.exitCode, output: it.output ?? (idx >= 0 ? chat[idx].meta?.output : undefined), paths: it.paths };
        if (idx >= 0) chat[idx] = { ...chat[idx], text: text || chat[idx].text, meta };
        else chat.push({ id: nextId(), role, text, meta });
      }
      return { ...st, chat };
    }
    case 'approval_request':
      chat.push({ id: nextId(), role: 'approval', text: ev.command ?? ev.kind, meta: { requestId: ev.requestId, reason: ev.reason } });
      return { ...st, chat, pending: [...st.pending, { requestId: ev.requestId, kind: 'approval', event: ev }] };
    case 'user_input_request':
      chat.push({ id: nextId(), role: 'question', text: ev.questions.map((q) => q.question).join('\n'), meta: { requestId: ev.requestId } });
      return { ...st, chat, pending: [...st.pending, { requestId: ev.requestId, kind: 'question', event: ev }] };
    case 'request_resolved':
      return { ...st, pending: st.pending.filter((p) => p.requestId !== ev.requestId) };
    case 'notice':
      chat.push({ id: nextId(), role: 'notice', text: ev.message, meta: { level: ev.level } });
      return { ...st, chat };
    case 'error':
      chat.push({ id: nextId(), role: 'notice', text: String(ev.message ?? 'error'), meta: { level: 'error' } });
      return { ...st, chat };
    case 'turn_diff':
      chat.push({ id: nextId(), role: 'diff', text: ev.diff });
      return {
        ...st,
        chat,
        lastDiff: { diff: ev.diff, baseSha: (ev as { baseSha?: string | null }).baseSha ?? null, files: (ev as { files?: string[] }).files ?? [] },
      };
    case 'turn_completed':
      chat.push({ id: nextId(), role: 'status', text: ev.status === 'completed' ? '本轮完成' : `本轮结束：${ev.status}${ev.error ? `（${ev.error}）` : ''}`, meta: { status: ev.status } });
      return { ...st, chat, running: false, pending: [] };
    default:
      return st;
  }
}

export const useWorkbenchStore = create<WorkbenchState>((set, get) => ({
  projectId: 0,
  iid: 0,
  flow: null,
  context: null,
  loading: false,
  error: null,
  tree: {},
  expanded: { '': true },
  buffers: {},
  tabs: [],
  active: null,
  stage: 'intent',
  stages: { intent: emptyStage(), spec: emptyStage(), plan: emptyStage() },
  connected: false,
  notices: [],
  aiTouched: {},
  revealLine: null,

  init: async (projectId, iid) => {
    get().dispose();
    const gen = ++sessionGen;
    set({
      projectId,
      iid,
      loading: true,
      error: null,
      flow: null,
      tree: {},
      expanded: { '': true },
      buffers: {},
      tabs: [],
      active: null,
      stages: { intent: emptyStage(), spec: emptyStage(), plan: emptyStage() },
      notices: [],
      aiTouched: {},
    });
    try {
      const flow = await api.getFlow(projectId, iid);
      get().applyFlow(flow);
      const ws = await api.ensureWorkspace(projectId, iid);
      set({ context: ws.context });
      await get().loadDir('');
      if (flow.primaryDir) {
        set((s) => ({ expanded: { ...s.expanded, [flow.primaryDir]: true } }));
        await get().loadDir(flow.primaryDir);
      }
      // Open the current stage document.
      const doc = flow.documents.find((d) => d.docType === get().stage) ?? flow.documents[flow.documents.length - 1];
      if (doc) await get().openFile(`${flow.writableDir}/${doc.docType}.md`);
      if (gen === sessionGen) connect(projectId, iid, gen);
    } catch (e) {
      set({ error: (e as Error).message });
    } finally {
      set({ loading: false });
    }
  },

  dispose: () => {
    sessionGen++;
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = null;
    // Flush pending autosaves instead of dropping them (verify B I1).
    const pending = Object.keys(autosaveTimers);
    Object.values(autosaveTimers).forEach(clearTimeout);
    autosaveTimers = {};
    pending.forEach((p) => void get().save(p, false));
    if (socket) {
      socket.onclose = null;
      socket.close();
    }
    socket = null;
    set({ connected: false });
  },

  applyFlow: (flow) => {
    const stage = currentStage(flow);
    set((s) => ({
      flow,
      // Follow the flow when its state changes (e.g. G1 → spec review),
      // keep the user's manual choice otherwise (verify B I2).
      stage: !s.flow || s.flow.state !== flow.state ? stage : s.stage,
      stages: {
        ...s.stages,
        ...(flow.runningStage ? { [flow.runningStage]: { ...s.stages[flow.runningStage], running: true } } : {}),
      },
    }));
  },

  refreshFlow: async () => {
    const { projectId, iid } = get();
    if (!projectId) return;
    try {
      get().applyFlow(await api.getFlow(projectId, iid));
    } catch (e) {
      get().pushNotice('error', (e as Error).message);
    }
  },

  loadDir: async (path) => {
    const { projectId, iid } = get();
    const entries = await api.listTree(projectId, iid, path);
    set((s) => ({ tree: { ...s.tree, [path]: entries } }));
  },

  toggleDir: async (path) => {
    const open = !get().expanded[path];
    set((s) => ({ expanded: { ...s.expanded, [path]: open } }));
    if (open && !get().tree[path]) {
      try {
        await get().loadDir(path);
      } catch (e) {
        get().pushNotice('error', (e as Error).message);
      }
    }
  },

  openFile: async (path) => {
    const { projectId, iid, buffers } = get();
    if (!buffers[path]) {
      try {
        const f: FileContent = await api.readFile(projectId, iid, path);
        set((s) => ({
          buffers: {
            ...s.buffers,
            [path]: {
              path,
              content: f.content ?? '',
              baseVersion: f.version || null,
              dirty: false,
              writable: f.writable,
              binary: f.binary,
              tooLarge: f.tooLarge,
              conflict: null,
            },
          },
        }));
      } catch (e) {
        get().pushNotice('error', (e as Error).message);
        return;
      }
    }
    set((s) => ({ tabs: s.tabs.includes(path) ? s.tabs : [...s.tabs, path], active: path }));
  },

  closeTab: (path) => {
    set((s) => {
      const tabs = s.tabs.filter((t) => t !== path);
      const buffers = { ...s.buffers };
      if (!buffers[path]?.dirty) delete buffers[path];
      return { tabs, buffers, active: s.active === path ? (tabs[tabs.length - 1] ?? null) : s.active };
    });
  },

  setActive: (path) => set({ active: path }),

  edit: (path, content) => {
    const buf = get().buffers[path];
    if (!buf || !buf.writable) return;
    set((s) => ({ buffers: { ...s.buffers, [path]: { ...buf, content, dirty: true } } }));
    if (autosaveTimers[path]) clearTimeout(autosaveTimers[path]);
    autosaveTimers[path] = setTimeout(() => {
      void get().save(path, false);
    }, AUTOSAVE_MS);
  },

  save: async (path, commit) => {
    const run = async () => {
      const { projectId, iid } = get();
      const buf = get().buffers[path];
      if (!buf || !buf.writable) return;
      if (!buf.dirty && !commit) return;
      if (autosaveTimers[path]) clearTimeout(autosaveTimers[path]);
      delete autosaveTimers[path];
      try {
        const r = await api.writeFile(projectId, iid, { path, content: buf.content, baseVersion: buf.baseVersion, commit });
        set((s) => {
          const cur = s.buffers[path];
          if (!cur) return {};
          return { buffers: { ...s.buffers, [path]: { ...cur, baseVersion: r.version, dirty: cur.content !== buf.content } } };
        });
        if (commit) get().pushNotice('info', r.commitSha ? `已提交 ${r.commitSha.slice(0, 8)}` : '没有需要提交的改动');
        await get().refreshFlow();
      } catch (e) {
        const msg = (e as Error).message;
        if (msg.includes('已被修改')) {
          try {
            const f = await api.readFile(projectId, iid, path);
            set((s) => (s.buffers[path] ? { buffers: { ...s.buffers, [path]: { ...s.buffers[path], conflict: { content: f.content ?? '', version: f.version } } } } : {}));
          } catch {
            /* ignore */
          }
        }
        get().pushNotice('error', msg);
      }
    };
    const prev = inflight[path] ?? Promise.resolve();
    const next = prev.then(run, run);
    inflight[path] = next;
    try {
      await next;
    } finally {
      if (inflight[path] === next) delete inflight[path];
    }
  },

  reloadFromDisk: async (path) => {
    const { projectId, iid } = get();
    const buf = get().buffers[path];
    if (!buf) return;
    try {
      const f = await api.readFile(projectId, iid, path);
      const upd = (fn: (b: Buffer) => Buffer) =>
        set((s) => (s.buffers[path] ? { buffers: { ...s.buffers, [path]: fn(s.buffers[path]) } } : {}));
      if (f.version === buf.baseVersion && !buf.conflict) {
        if (buf.writable !== f.writable) upd((b) => ({ ...b, writable: f.writable }));
        return;
      }
      if (get().buffers[path]?.dirty) {
        upd((b) => ({ ...b, conflict: { content: f.content ?? '', version: f.version } }));
      } else {
        upd((b) => ({ ...b, content: f.content ?? '', baseVersion: f.version, writable: f.writable, conflict: null }));
      }
    } catch {
      /* deleted / unreadable: keep buffer */
    }
  },

  resolveConflict: (path, keep) => {
    const buf = get().buffers[path];
    if (!buf?.conflict) return;
    const c = buf.conflict;
    if (keep === 'theirs') {
      set((s) => ({ buffers: { ...s.buffers, [path]: { ...buf, content: c.content, baseVersion: c.version, dirty: false, conflict: null } } }));
    } else {
      // Keep my content on top of the newer version.
      set((s) => ({ buffers: { ...s.buffers, [path]: { ...buf, baseVersion: c.version, dirty: true, conflict: null } } }));
      void get().save(path, false);
    }
  },

  setStage: (stage) => set({ stage }),

  send: (text, extra) => {
    const { stage } = get();
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      get().pushNotice('error', 'AI 连接未就绪，请稍后重试');
      return false;
    }
    socket.send(JSON.stringify({ type: 'send', stage, text, files: extra?.files, selection: extra?.selection }));
    return true;
  },

  steer: (text) => {
    socket?.send(JSON.stringify({ type: 'steer', stage: get().stage, text }));
  },

  interrupt: () => {
    socket?.send(JSON.stringify({ type: 'interrupt', stage: get().stage }));
  },

  answer: (requestId, answer) => {
    socket?.send(JSON.stringify({ type: 'answer', stage: get().stage, requestId, answer }));
    set((s) => ({
      stages: { ...s.stages, [s.stage]: { ...s.stages[s.stage], pending: s.stages[s.stage].pending.filter((p) => p.requestId !== requestId) } },
    }));
  },

  pushNotice: (level, message) => set((s) => ({ notices: [...s.notices.slice(-4), { id: ++noticeSeq, level, message }] })),
  dismissNotice: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),
}));

/** Stage to focus for a flow state. Exported for tests. */
export function currentStage(flow: FlowView): DocType {
  switch (flow.state) {
    case 'intent':
    case 'intent_review':
      return 'intent';
    case 'spec':
    case 'spec_review':
      return 'spec';
    default:
      return flow.documents.some((d) => d.docType === 'plan') ? 'plan' : 'intent';
  }
}

function connect(projectId: number, iid: number, gen: number) {
  const { setState } = useWorkbenchStore;
  const alive = () => gen === sessionGen;
  api
    .wsTicket(projectId, iid)
    .then((ticket) => {
      if (!alive()) return;
      if (socket) {
        socket.onclose = null;
        socket.close();
      }
      const ws = new WebSocket(api.wsUrl(projectId, iid, ticket));
      socket = ws;
      ws.onopen = () => {
        if (alive()) setState({ connected: true });
      };
      ws.onclose = () => {
        if (socket === ws) socket = null;
        if (!alive()) return;
        setState({ connected: false });
        reconnectTimer = setTimeout(() => connect(projectId, iid, gen), 3000);
      };
      ws.onmessage = (m) => {
        if (alive() && socket === ws) handleMessage(JSON.parse(String(m.data)));
      };
    })
    .catch(() => {
      if (alive()) reconnectTimer = setTimeout(() => connect(projectId, iid, gen), 5000);
    });
}

/** Dispatch one server message. Exported for tests. */
export function handleMessage(msg: Record<string, unknown>) {
  const { getState, setState } = useWorkbenchStore;
  const type = msg.type as string;
  if (type === 'history') {
    const stage = msg.stage as DocType;
    let st = emptyStage();
    for (const ev of (msg.events as AgentEvent[]) ?? []) st = reduceAgentEvent(st, ev);
    setState((s) => ({ stages: { ...s.stages, [stage]: { ...st, running: s.stages[stage].running } } }));
  } else if (type === 'agent') {
    const stage = msg.stage as DocType;
    const ev = msg.event as AgentEvent;
    setState((s) => ({ stages: { ...s.stages, [stage]: reduceAgentEvent(s.stages[stage], ev) } }));
    if (ev.type === 'turn_diff') {
      const files = (ev as { files?: string[] }).files ?? [];
      const touched: Record<string, boolean> = {};
      for (const f of files) {
        const parts = f.split('/');
        for (let i = 1; i <= parts.length; i++) touched[parts.slice(0, i).join('/')] = true;
      }
      setState(() => ({ aiTouched: touched }));
      files.forEach((f) => void getState().reloadFromDisk(f));
    }
  } else if (type === 'agent_state') {
    const stage = msg.stage as DocType;
    setState((s) => ({ stages: { ...s.stages, [stage]: { ...s.stages[stage], running: Boolean(msg.running) } } }));
  } else if (type === 'flow_changed') {
    void getState().refreshFlow();
  } else if (type === 'file_changed' || type === 'fs_changed') {
    const paths = type === 'fs_changed' ? (msg.paths as string[]) : [msg.path as string];
    const { buffers, tree } = getState();
    paths.filter((p) => buffers[p]).forEach((p) => void getState().reloadFromDisk(p));
    // Refresh listed directories containing changed paths.
    const dirs = new Set(paths.map((p) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '')));
    dirs.forEach((d) => {
      if (tree[d]) void getState().loadDir(d).catch(() => undefined);
    });
  } else if (type === 'notice') {
    getState().pushNotice(String(msg.level ?? 'info'), String(msg.message ?? ''));
  } else if (type === 'error') {
    getState().pushNotice('error', String(msg.message ?? ''));
  } else if (type === 'tree_changed') {
    Object.keys(getState().tree).forEach((d) => void getState().loadDir(d).catch(() => undefined));
  }
}
