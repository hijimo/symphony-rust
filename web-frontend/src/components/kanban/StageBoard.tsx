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
import { startFast, startFlow } from '../../api/changes';

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

/** "3 小时" style wait duration since `iso`; empty when unparseable. */
export function waitedFor(iso: string, now = Date.now()): string {
  const t = Date.parse(iso.includes('T') ? iso : iso.replace(' ', 'T') + 'Z');
  if (Number.isNaN(t)) return '';
  const min = Math.max(0, Math.floor((now - t) / 60000));
  if (min < 60) return `${min} 分钟`;
  if (min < 60 * 24) return `${Math.floor(min / 60)} 小时`;
  return `${Math.floor(min / 1440)} 天`;
}

const SUB_STATE_COLOR: Record<string, 'error' | 'info' | 'success'> = {
  合并失败: 'error',
  '等待 CI': 'info',
  已批准待合并: 'success',
};

function Card({ card, projectId, onStart }: { card: StageIssueCard; projectId: number; onStart: (c: StageIssueCard) => void }) {
  const navigate = useNavigate();
  const p = card.placement;
  const pid = card.projectId ?? projectId;
  const reviewSince = card.humanReviewSince ?? card.flow?.reviewSince;
  const reviewWait = reviewSince ? waitedFor(reviewSince) : '';
  const drafting = card.flow?.documents.some((d) => d.draftState === 'running');
  const runningFor = card.aiRunning ? waitedFor(card.aiRunning.since) : '';
  return (
    <Paper variant="outlined" sx={{ p: 1, borderRadius: '6px' }} data-testid={`stage-card-${card.iid}`}>
      <Box sx={{ display: 'flex', gap: 0.5, alignItems: 'center', mb: 0.5, flexWrap: 'wrap' }}>
        {card.projectName && (
          <Chip size="small" variant="outlined" label={card.projectName} sx={{ height: 20, fontSize: 10, color: '#0053db' }} />
        )}
        <Tooltip title={p.subState === '合并失败' ? (card.flow?.mergeError ?? '') : ''}>
          <Chip
            size="small"
            label={p.subState}
            sx={{ height: 20, fontSize: 11 }}
            color={SUB_STATE_COLOR[p.subState] ?? (p.waitingHuman ? 'warning' : 'default')}
          />
        </Tooltip>
        {p.waitingHuman && (
          <Typography sx={{ fontSize: 11, color: '#ac3500' }}>
            等待审批{reviewWait ? ` · ${reviewWait}` : ''}
          </Typography>
        )}
        {(drafting || card.aiRunning) && (
          <Typography sx={{ fontSize: 11, color: '#0053db' }}>AI 运行中{runningFor ? ` · ${runningFor}` : ''}</Typography>
        )}
        {card.track && <Chip size="small" variant="outlined" label={`track:${card.track}`} sx={{ height: 20, fontSize: 10 }} />}
        {p.anomaly && (
          <Tooltip title={p.anomalyReason ?? 'Issue 同时带有多个流程 label'}>
            <WarningAmberIcon sx={{ fontSize: 14, color: '#ba1a1a' }} aria-label="label 异常" />
          </Tooltip>
        )}
      </Box>
      <Link
        component="button"
        onClick={() => navigate(card.flow ? `/projects/${pid}/changes/${card.iid}` : `/projects/${pid}/issues/${card.iid}`)}
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
          {!card.flow && ['plan', 'build'].includes(card.placement.column) && !card.placement.anomaly &&
            ['待启动', '排队'].includes(card.placement.subState) && (
            <Button size="small" sx={{ fontSize: 11, py: 0, minWidth: 0 }} onClick={() => onStart(card)}>
              开始文档流程
            </Button>
          )}
        </Box>
      </Box>
    </Paper>
  );
}

/**
 * Stage columns. `projectId` is the default project; cards/MRs carrying
 * their own `projectId` (global overview board) route to that project.
 */
export default function StageBoard({
  data,
  projectId,
  onChanged,
}: {
  data: Pick<StageKanban, 'columns'> & Partial<Pick<StageKanban, 'prError' | 'fastTrackLabels'>>;
  projectId: number;
  onChanged: () => void;
}) {
  const navigate = useNavigate();
  const [starting, setStarting] = useState<StageIssueCard | null>(null);
  const [track, setTrack] = useState<Track | 'fast'>('plan');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');
  const fastAllowed = Boolean(
    starting?.labels.some((l) => (data.fastTrackLabels ?? []).includes(l.trim().toLowerCase())),
  );

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
                <Card key={`${i.projectId ?? projectId}-${i.iid}`} card={i} projectId={projectId} onStart={setStarting} />
              ))}
              {c.mergeRequests.map((mr) => (
                <Paper key={`mr-${mr.projectId ?? projectId}-${mr.iid}`} variant="outlined" sx={{ p: 1 }}>
                  {mr.projectName && <Typography sx={{ fontSize: 10, color: '#0053db' }}>{mr.projectName}</Typography>}
                  <Link component="button" onClick={() => navigate(`/projects/${mr.projectId ?? projectId}/mrs/${mr.iid}`)} sx={{ fontSize: 13, textAlign: 'left' }}>
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
          <TextField select fullWidth size="small" label="路径" value={track} onChange={(e) => setTrack(e.target.value as Track | 'fast')} sx={{ mt: 1 }}>
            <MenuItem value="plan">track:plan（只写 plan，默认）</MenuItem>
            <MenuItem value="full">track:full（intent → spec → plan）</MenuItem>
            <MenuItem value="fast" disabled={!fastAllowed}>
              track:fast（不写文档，直接进入实现）
            </MenuItem>
          </TextField>
          {!fastAllowed && (
            <Typography sx={{ fontSize: 12, color: '#737686', mt: 0.5 }}>
              track:fast 只用于带 {(data.fastTrackLabels ?? []).join('、') || '白名单'} label 的 Issue
            </Typography>
          )}
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
                const pid = starting.projectId ?? projectId;
                const iid = starting.iid;
                if (track === 'fast') {
                  await startFast(pid, iid);
                  setStarting(null);
                  onChanged();
                  return;
                }
                await startFlow(pid, { track, issueIid: iid });
                setStarting(null);
                onChanged();
                navigate(`/projects/${pid}/changes/${iid}`);
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
