import { useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Link,
  MenuItem,
  Paper,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material';
import WarningAmberIcon from '@mui/icons-material/WarningAmber';
import { useNavigate } from 'react-router-dom';
import type { DocStatus, StageIssueCard, StageKanban, Track } from '../../types/changes';
import { startFlow } from '../../api/changes';

const DOC_ICON: Record<DocStatus | 'none', string> = {
  none: '○',
  draft: '✎',
  in_review: '◐',
  changes_requested: '✗',
  approved: '●',
  stale: '↻',
  rejected: '⊘',
  merged: '✔',
};

function DocProgress({ card }: { card: StageIssueCard }) {
  if (!card.flow) return null;
  const docs = card.flow.track === 'plan' ? (['plan'] as const) : (['intent', 'spec', 'plan'] as const);
  return (
    <Box sx={{ display: 'flex', gap: 0.5 }} aria-label="文档进度">
      {docs.map((d) => {
        const doc = card.flow!.documents.find((x) => x.docType === d);
        const st = doc?.status ?? 'none';
        return (
          <Tooltip key={d} title={`${d}: ${st}${doc?.draftState === 'failed' ? '（起草失败）' : ''}`}>
            <Typography component="span" sx={{ fontSize: 11, color: doc?.draftState === 'failed' ? '#ba1a1a' : '#434655' }}>
              {d[0].toUpperCase()}
              {DOC_ICON[st]}
            </Typography>
          </Tooltip>
        );
      })}
    </Box>
  );
}

function Card({ card, projectId, onStart }: { card: StageIssueCard; projectId: number; onStart: (c: StageIssueCard) => void }) {
  const navigate = useNavigate();
  const p = card.placement;
  return (
    <Paper variant="outlined" sx={{ p: 1, borderRadius: '6px' }} data-testid={`stage-card-${card.iid}`}>
      <Box sx={{ display: 'flex', gap: 0.5, alignItems: 'center', mb: 0.5, flexWrap: 'wrap' }}>
        <Chip size="small" label={p.subState} sx={{ height: 20, fontSize: 11 }} color={p.waitingHuman ? 'warning' : 'default'} />
        {p.waitingHuman && <Typography sx={{ fontSize: 11, color: '#ac3500' }}>等待审批</Typography>}
        {card.flow?.documents.some((d) => d.draftState === 'running') && <Typography sx={{ fontSize: 11, color: '#0053db' }}>AI 运行中</Typography>}
        {card.track && <Chip size="small" variant="outlined" label={`track:${card.track}`} sx={{ height: 20, fontSize: 10 }} />}
        {p.anomaly && (
          <Tooltip title="Issue 同时带有多个流程 label">
            <WarningAmberIcon sx={{ fontSize: 14, color: '#ba1a1a' }} aria-label="label 异常" />
          </Tooltip>
        )}
      </Box>
      <Link
        component="button"
        onClick={() => navigate(card.flow ? `/projects/${projectId}/changes/${card.iid}` : `/projects/${projectId}/issues/${card.iid}`)}
        sx={{ fontSize: 13, textAlign: 'left', color: '#191b23' }}
      >
        #{card.iid} {card.title}
      </Link>
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mt: 0.5 }}>
        <DocProgress card={card} />
        <Box sx={{ display: 'flex', gap: 1 }}>
          {card.flow?.docsMrUrl && (
            <Link href={card.flow.docsMrUrl} target="_blank" rel="noreferrer" sx={{ fontSize: 11 }}>
              文档 MR
            </Link>
          )}
          {!card.flow && card.placement.subState === '待启动' && (
            <Button size="small" sx={{ fontSize: 11, py: 0, minWidth: 0 }} onClick={() => onStart(card)}>
              开始文档流程
            </Button>
          )}
        </Box>
      </Box>
    </Paper>
  );
}

export default function StageBoard({ data, projectId, onChanged }: { data: StageKanban; projectId: number; onChanged: () => void }) {
  const navigate = useNavigate();
  const [starting, setStarting] = useState<StageIssueCard | null>(null);
  const [track, setTrack] = useState<Track>('plan');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  return (
    <>
      {data.prError && <Alert severity="warning" sx={{ mb: 1 }}>MR 加载失败：{data.prError}</Alert>}
      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: 'repeat(5, minmax(0, 1fr))' }, gap: 1.5, alignItems: 'start' }}>
        {data.columns.map((c) => (
          <Box key={c.key} data-testid={`stage-col-${c.key}`} sx={{ bgcolor: '#f3f3fe', borderRadius: '8px', p: 1 }}>
            <Box sx={{ mb: 1 }}>
              <Typography sx={{ fontWeight: 600, fontSize: 14 }}>
                {c.title} <span style={{ color: '#737686', fontWeight: 400 }}>{c.totalCount}</span>
              </Typography>
              <Typography sx={{ fontSize: 11, color: '#737686' }}>{c.hint}</Typography>
            </Box>
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              {c.issues.map((i) => (
                <Card key={i.iid} card={i} projectId={projectId} onStart={setStarting} />
              ))}
              {c.mergeRequests.map((mr) => (
                <Paper key={`mr-${mr.iid}`} variant="outlined" sx={{ p: 1 }}>
                  <Link component="button" onClick={() => navigate(`/projects/${projectId}/mrs/${mr.iid}`)} sx={{ fontSize: 13, textAlign: 'left' }}>
                    !{mr.iid} {mr.title}
                  </Link>
                  <Typography sx={{ fontSize: 11, color: '#737686' }}>{mr.source_branch} · CI {mr.ci_status ?? '-'}</Typography>
                </Paper>
              ))}
              {c.issues.length === 0 && c.mergeRequests.length === 0 && (
                <Typography sx={{ fontSize: 12, color: '#9aa0b4', textAlign: 'center', py: 2 }}>暂无</Typography>
              )}
            </Box>
          </Box>
        ))}
      </Box>
      <Dialog open={Boolean(starting)} onClose={() => setStarting(null)} fullWidth>
        <DialogTitle>开始文档流程 · #{starting?.iid}</DialogTitle>
        <DialogContent>
          <TextField select fullWidth size="small" label="路径" value={track} onChange={(e) => setTrack(e.target.value as Track)} sx={{ mt: 1 }}>
            <MenuItem value="plan">track:plan（只写 plan，默认）</MenuItem>
            <MenuItem value="full">track:full（intent → spec → plan）</MenuItem>
          </TextField>
          {err && <Alert severity="error" sx={{ mt: 1 }}>{err}</Alert>}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setStarting(null)}>取消</Button>
          <Button
            variant="contained"
            disabled={busy}
            onClick={async () => {
              if (!starting) return;
              setBusy(true);
              setErr('');
              try {
                await startFlow(projectId, { track, issueIid: starting.iid });
                const iid = starting.iid;
                setStarting(null);
                onChanged();
                navigate(`/projects/${projectId}/changes/${iid}`);
              } catch (e) {
                setErr((e as Error).message);
              } finally {
                setBusy(false);
              }
            }}
          >
            开始
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
