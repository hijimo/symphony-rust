import { useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Collapse,
  IconButton,
  Paper,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Tooltip,
  Typography,
} from '@mui/material';
import SendIcon from '@mui/icons-material/Send';
import StopIcon from '@mui/icons-material/Stop';
import TerminalIcon from '@mui/icons-material/Terminal';
import DescriptionIcon from '@mui/icons-material/Description';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import { useWorkbenchStore, type ChatEntry, type SelectionRef } from '../../store/workbenchStore';
import * as api from '../../api/changes';
import { diffHunks, revertHunk } from '../../utils/hunks';
import { setLastDiffOriginal } from './EditorPane';
import type { DocType } from '../../types/changes';

const STAGE_LABEL: Record<DocType, string> = { intent: 'Intent', spec: 'Spec', plan: 'Plan' };

const PATH_RE = /^`([^`\s]+\.[a-zA-Z0-9]+)(?::(\d+))?`$/;

/** Parse `` `path.ext:12` `` references in AI messages. Exported for tests. */
export function parseFileRef(token: string): { path: string; line: number | null } | null {
  const m = PATH_RE.exec(token);
  return m ? { path: m[1], line: m[2] ? Number(m[2]) : null } : null;
}

function FileLinks({ text }: { text: string }) {
  const openFile = useWorkbenchStore((s) => s.openFile);
  const parts = text.split(/(`[^`\s]+\.[a-zA-Z0-9]+(?::\d+)?`)/g);
  return (
    <>
      {parts.map((p, i) =>
        parseFileRef(p) ? (
          <Box
            key={i}
            component="a"
            href="#"
            onClick={(e: React.MouseEvent) => {
              e.preventDefault();
              const ref = parseFileRef(p)!;
              void openFile(ref.path).then(() => {
                if (ref.line) useWorkbenchStore.setState({ revealLine: { path: ref.path, line: ref.line } });
              });
            }}
            sx={{ fontFamily: 'monospace', color: '#0053db' }}
          >
            {p.slice(1, -1)}
          </Box>
        ) : (
          <span key={i}>{p}</span>
        ),
      )}
    </>
  );
}

function Entry({ e }: { e: ChatEntry }) {
  const [open, setOpen] = useState(false);
  switch (e.role) {
    case 'user':
      return (
        <Paper elevation={0} sx={{ p: 1, bgcolor: '#e8eefc', alignSelf: 'flex-end', maxWidth: '90%' }}>
          <Typography sx={{ fontSize: 13, whiteSpace: 'pre-wrap' }}>{e.text.split('\n\nReferenced files')[0].split('\n\nSelected text')[0]}</Typography>
          {e.meta?.mode === 'non_interactive' && <Chip size="small" label="自动起草" sx={{ mt: 0.5 }} />}
        </Paper>
      );
    case 'assistant':
      return (
        <Typography sx={{ fontSize: 13, whiteSpace: 'pre-wrap' }} data-testid="assistant-message">
          <FileLinks text={e.text} />
        </Typography>
      );
    case 'reasoning':
      return <Typography sx={{ fontSize: 12, color: '#737686', fontStyle: 'italic', whiteSpace: 'pre-wrap' }}>{e.text}</Typography>;
    case 'command':
    case 'file_change':
      return (
        <Paper variant="outlined" sx={{ p: 0.5 }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, cursor: 'pointer' }} onClick={() => setOpen(!open)}>
            {e.role === 'command' ? <TerminalIcon sx={{ fontSize: 14 }} /> : <DescriptionIcon sx={{ fontSize: 14 }} />}
            <Typography noWrap sx={{ fontSize: 12, fontFamily: 'monospace', flex: 1 }}>
              {e.role === 'command' ? e.text : `修改 ${e.text}`}
            </Typography>
            <Typography sx={{ fontSize: 11, color: '#737686' }}>{String(e.meta?.status ?? '')}</Typography>
            <ExpandMoreIcon sx={{ fontSize: 14 }} />
          </Box>
          <Collapse in={open}>
            <Box component="pre" sx={{ fontSize: 11, m: 0, mt: 0.5, maxHeight: 200, overflow: 'auto' }}>
              {String(e.meta?.output ?? '')}
            </Box>
          </Collapse>
        </Paper>
      );
    case 'notice':
      return (
        <Alert severity={(e.meta?.level as 'info' | 'warning' | 'error') ?? 'info'} sx={{ py: 0, fontSize: 12 }}>
          {e.text}
        </Alert>
      );
    case 'status':
      return <Typography sx={{ fontSize: 11, color: '#737686', textAlign: 'center' }}>— {e.text} —</Typography>;
    case 'diff':
      return (
        <Paper variant="outlined" sx={{ p: 0.5 }}>
          <Box sx={{ display: 'flex', alignItems: 'center', cursor: 'pointer' }} onClick={() => setOpen(!open)}>
            <Typography sx={{ fontSize: 12, flex: 1 }}>本轮改动 diff</Typography>
            <ExpandMoreIcon sx={{ fontSize: 14 }} />
          </Box>
          <Collapse in={open}>
            <Box component="pre" sx={{ fontSize: 11, m: 0, maxHeight: 240, overflow: 'auto' }}>
              {e.text}
            </Box>
          </Collapse>
        </Paper>
      );
    default:
      return null;
  }
}

function PendingCard() {
  const { stages, stage, answer } = useWorkbenchStore();
  const p = stages[stage].pending[0];
  const [answers, setAnswers] = useState<Record<string, string>>({});
  useEffect(() => setAnswers({}), [p?.requestId]);
  if (!p) return null;
  if (p.kind === 'approval' && p.event.type === 'approval_request') {
    return (
      <Alert
        severity="warning"
        data-testid="approval-card"
        action={
          <>
            <Button size="small" onClick={() => answer(p.requestId, { decision: 'accept' })}>允许</Button>
            <Button size="small" color="error" onClick={() => answer(p.requestId, { decision: 'decline' })}>拒绝</Button>
          </>
        }
      >
        AI 请求执行命令：<code>{p.event.command}</code>
      </Alert>
    );
  }
  if (p.kind === 'question' && p.event.type === 'user_input_request') {
    const qs = p.event.questions;
    return (
      <Paper variant="outlined" sx={{ p: 1 }} data-testid="question-card">
        {qs.map((q) => (
          <Box key={q.id} sx={{ mb: 1 }}>
            <Typography sx={{ fontSize: 13, fontWeight: 600 }}>{q.header}</Typography>
            <Typography sx={{ fontSize: 13 }}>{q.question}</Typography>
            {q.options?.map((o) => (
              <Chip key={o.label} label={o.label} size="small" sx={{ mr: 0.5, mt: 0.5 }}
                color={answers[q.id] === o.label ? 'primary' : 'default'}
                onClick={() => setAnswers({ ...answers, [q.id]: o.label })} />
            ))}
            <TextField size="small" fullWidth placeholder="或直接输入回答" value={answers[q.id] ?? ''}
              onChange={(e) => setAnswers({ ...answers, [q.id]: e.target.value })} sx={{ mt: 0.5 }} />
          </Box>
        ))}
        <Button size="small" variant="contained"
          onClick={() => answer(p.requestId, { answers: Object.fromEntries(qs.map((q) => [q.id, { answers: [answers[q.id] ?? ''] }])) })}>
          回答
        </Button>
      </Paper>
    );
  }
  return null;
}

/** Review AI changes of the last turn: accept, revert all or revert hunks. */
function TurnReview() {
  const { stages, stage, projectId, iid, buffers, openFile, save, edit } = useWorkbenchStore();
  const diff = stages[stage].lastDiff;
  const [before, setBefore] = useState<Record<string, string | null>>({});
  const [dismissed, setDismissed] = useState<string | null>(null);
  const key = diff ? `${diff.baseSha}:${diff.files.join(',')}` : null;

  useEffect(() => {
    if (!diff?.baseSha) return;
    let cancelled = false;
    Promise.all(
      diff.files.map(async (f) => {
        const r = await api.readFileAt(projectId, iid, f, diff.baseSha!).catch(() => null);
        return [f, r] as const;
      }),
    ).then((pairs) => {
      if (cancelled) return;
      const m = Object.fromEntries(pairs);
      setBefore(m);
      pairs.forEach(([f, c]) => setLastDiffOriginal(f, c ?? ''));
    });
    return () => {
      cancelled = true;
    };
  }, [diff?.baseSha, diff?.files, projectId, iid]);

  const hunks = useMemo(() => {
    if (!diff) return [];
    return diff.files.flatMap((f) => {
      const b = before[f];
      const after = buffers[f]?.content;
      if (b === undefined || after === undefined) return [];
      return diffHunks(b ?? '', after).map((h, i) => ({ file: f, h, i }));
    });
  }, [diff, before, buffers]);

  if (!diff || !diff.files.length || dismissed === key) return null;
  const revertAll = async () => {
    for (const f of diff.files) {
      if (!buffers[f]) await openFile(f);
      const b = before[f];
      if (b != null) {
        edit(f, b);
        await save(f, true);
      }
    }
    setDismissed(key);
  };
  return (
    <Paper variant="outlined" sx={{ p: 1, borderColor: '#ac3500' }} data-testid="turn-review">
      <Typography sx={{ fontSize: 12, fontWeight: 600 }}>AI 本轮修改了 {diff.files.length} 个文件，共 {hunks.length} 处</Typography>
      {hunks.map(({ file, h, i }) => (
        <Box key={`${file}:${i}`} sx={{ mt: 0.5, borderLeft: '3px solid #ac3500', pl: 1 }}>
          <Typography sx={{ fontSize: 11, color: '#737686' }}>
            {file} · 第 {h.afterStart + 1} 行
          </Typography>
          <Box component="pre" sx={{ fontSize: 11, m: 0, maxHeight: 120, overflow: 'auto' }}>
            {h.beforeLines.map((l) => `- ${l}`).join('\n')}
            {h.beforeLines.length ? '\n' : ''}
            {h.afterLines.map((l) => `+ ${l}`).join('\n')}
          </Box>
          <Button
            size="small"
            onClick={async () => {
              const cur = buffers[file]?.content ?? '';
              edit(file, revertHunk(cur, h));
              await save(file, true);
            }}
          >
            回退这一处
          </Button>
        </Box>
      ))}
      <Box sx={{ mt: 1, display: 'flex', gap: 1 }}>
        <Button size="small" variant="contained" onClick={() => setDismissed(key)}>接受全部</Button>
        <Button size="small" color="error" onClick={revertAll}>整轮回退</Button>
      </Box>
    </Paper>
  );
}

export default function AiPanel({ selection, onClearSelection }: { selection: SelectionRef | null; onClearSelection: () => void }) {
  const { stage, setStage, stages, send, steer, interrupt, connected, flow, active } = useWorkbenchStore();
  const [text, setText] = useState('');
  const [files, setFiles] = useState<string[]>([]);
  const st = stages[stage];
  const stagesAvailable = (flow?.documents.map((d) => d.docType) ?? []) as DocType[];

  const submit = () => {
    const t = text.trim();
    if (!t) return;
    if (st.running) {
      steer(t);
    } else if (!send(t, { files, selection: selection ?? undefined })) {
      return;
    }
    setText('');
    setFiles([]);
    onClearSelection();
  };

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', bgcolor: '#fff' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', p: 0.5, borderBottom: '1px solid #e7e7f3', gap: 1 }}>
        <ToggleButtonGroup size="small" exclusive value={stage} onChange={(_, v) => v && setStage(v)}>
          {(['intent', 'spec', 'plan'] as DocType[]).map((d) => (
            <ToggleButton key={d} value={d} disabled={!stagesAvailable.includes(d)} sx={{ py: 0.2, fontSize: 11 }}>
              {STAGE_LABEL[d]}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
        <Box sx={{ flex: 1 }} />
        <Tooltip title={connected ? 'AI 已连接' : 'AI 未连接'}>
          <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: connected ? '#2e7d32' : '#c3c6d7' }} />
        </Tooltip>
        {st.running && <CircularProgress size={14} aria-label="AI 运行中" />}
      </Box>
      <Box sx={{ flex: 1, overflow: 'auto', p: 1, display: 'flex', flexDirection: 'column', gap: 1 }} data-testid="chat">
        {st.chat.length === 0 && (
          <Typography sx={{ fontSize: 12, color: '#737686' }}>
            和 AI 讨论 {STAGE_LABEL[stage]}：它能读整个仓库，只会修改 {flow?.writableDir ?? '文档目录'} 下的文档。
          </Typography>
        )}
        {st.chat.map((e) => (
          <Entry key={e.id} e={e} />
        ))}
        <PendingCard />
        {!st.running && <TurnReview />}
      </Box>
      <Box sx={{ p: 1, borderTop: '1px solid #e7e7f3' }}>
        {(selection || files.length > 0) && (
          <Box sx={{ mb: 0.5, display: 'flex', flexWrap: 'wrap', gap: 0.5 }}>
            {selection && <Chip size="small" label={`选区 ${selection.path.split('/').pop()}:${selection.startLine}-${selection.endLine}`} onDelete={onClearSelection} />}
            {files.map((f) => (
              <Chip key={f} size="small" label={`@${f}`} onDelete={() => setFiles(files.filter((x) => x !== f))} />
            ))}
          </Box>
        )}
        <TextField
          multiline
          minRows={2}
          maxRows={8}
          fullWidth
          size="small"
          placeholder={st.running ? '追加说明（AI 运行中）' : '输入消息，@ 引用当前文件，Ctrl+Enter 发送'}
          value={text}
          onChange={(e) => {
            const v = e.target.value;
            if (v.endsWith('@') && active && !files.includes(active)) {
              setFiles([...files, active]);
              setText(v.slice(0, -1));
              return;
            }
            setText(v);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              submit();
            }
          }}
          inputProps={{ 'aria-label': 'AI 输入' }}
        />
        <Box sx={{ display: 'flex', justifyContent: 'flex-end', gap: 1, mt: 0.5 }}>
          {st.running && (
            <IconButton size="small" color="error" onClick={interrupt} aria-label="停止">
              <StopIcon />
            </IconButton>
          )}
          <Button size="small" variant="contained" endIcon={<SendIcon />} onClick={submit} disabled={!text.trim()}>
            {st.running ? '追加说明' : '发送'}
          </Button>
        </Box>
      </Box>
    </Box>
  );
}
