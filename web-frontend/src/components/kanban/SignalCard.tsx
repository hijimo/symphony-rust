import { useState } from 'react';
import { Box, Button, Chip, Drawer, Link, Menu, MenuItem, Paper, Typography } from '@mui/material';
import { useNavigate } from 'react-router-dom';
import { createSignalIntent, ignoreSignal } from '../../api/signals';
import type { SignalRecord, SignalSeverity } from '../../types/signals';
import { waitedFor } from './StageBoard';

const SEVERITY: Record<SignalSeverity, { label: string; color: 'error' | 'warning' | 'default' }> = {
  critical: { label: '严重', color: 'error' },
  warning: { label: '警告', color: 'warning' },
  info: { label: '提示', color: 'default' },
};

function SignalDetail({ s, onClose }: { s: SignalRecord; onClose: () => void }) {
  const details = s.evidence?.details;
  return (
    <Drawer anchor="right" open onClose={onClose}>
      <Box data-testid="signal-detail" sx={{ width: 460, p: 2, display: 'flex', flexDirection: 'column', gap: 1 }}>
        <Typography sx={{ fontWeight: 600 }}>{s.title}</Typography>
        <Typography sx={{ fontSize: 12, color: '#737686' }}>
          {s.source} · {SEVERITY[s.severity].label} · 触发 {s.count} 次 · 首次 {s.firstSeenAt} · 最近 {s.lastSeenAt}
        </Typography>
        {s.externalUrl && (
          <Link href={s.externalUrl} target="_blank" rel="noreferrer" sx={{ fontSize: 12 }}>
            在来源中查看
          </Link>
        )}
        {s.evidence?.message && (
          <Typography sx={{ fontSize: 13, whiteSpace: 'pre-wrap' }}>{s.evidence.message}</Typography>
        )}
        {details != null && (
          <Box component="pre" sx={{ fontSize: 11, bgcolor: '#f3f3fe', p: 1, borderRadius: 1, overflow: 'auto', m: 0 }}>
            {JSON.stringify(details, null, 2)}
          </Box>
        )}
        <Typography sx={{ fontSize: 11, color: '#9aa0b4' }}>内容来自外部系统，已脱敏。</Typography>
      </Box>
    </Drawer>
  );
}

/** Untriaged signal in 运维 (P3 §3 S3). */
export default function SignalCard({ s, onChanged }: { s: SignalRecord; onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const navigate = useNavigate();
  const sev = SEVERITY[s.severity];
  const generate = async () => {
    setBusy(true);
    setError('');
    try {
      const v = await createSignalIntent(s.projectId, s.id);
      onChanged();
      navigate(`/projects/${s.projectId}/changes/${v.flow.issueIid}`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const doIgnore = async (days: 1 | 7 | null) => {
    setMenu(null);
    setError('');
    try {
      await ignoreSignal(s.projectId, s.id, days);
      onChanged();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const since = waitedFor(s.lastSeenAt);
  return (
    <Paper variant="outlined" sx={{ p: 1, borderRadius: '6px', borderColor: s.severity === 'critical' ? '#f2b8b5' : undefined }} data-testid={`signal-card-${s.id}`}>
      <Box sx={{ display: 'flex', gap: 0.5, alignItems: 'center', flexWrap: 'wrap', mb: 0.5 }}>
        {s.projectName && <Chip size="small" variant="outlined" label={s.projectName} sx={{ height: 20, fontSize: 10, color: '#0053db' }} />}
        <Chip size="small" label={sev.label} color={sev.color} sx={{ height: 20, fontSize: 11 }} />
        <Chip size="small" variant="outlined" label={s.source} sx={{ height: 20, fontSize: 10 }} />
        {s.resolved && <Chip size="small" variant="outlined" color="success" label="已恢复" sx={{ height: 20, fontSize: 10 }} />}
        {s.issueIid != null && (
          <Chip size="small" variant="outlined" color="warning" label={`复发 · 原 #${s.issueIid}`} sx={{ height: 20, fontSize: 10 }} />
        )}
      </Box>
      <Link component="button" onClick={() => setOpen(true)} sx={{ fontSize: 13, textAlign: 'left', color: '#191b23' }}>
        {s.title}
      </Link>
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mt: 0.5 }}>
        <Typography sx={{ fontSize: 11, color: '#737686' }}>
          触发 {s.count} 次{since ? ` · ${since}前` : ''}
        </Typography>
        <Box sx={{ display: 'flex', gap: 1, alignItems: 'center' }}>
          {s.externalUrl && (
            <Link href={s.externalUrl} target="_blank" rel="noreferrer" sx={{ fontSize: 11 }}>
              来源
            </Link>
          )}
          <Button size="small" sx={{ fontSize: 11, py: 0, minWidth: 0 }} disabled={busy} onClick={(e) => setMenu(e.currentTarget)}>
            忽略
          </Button>
          <Button size="small" variant="contained" sx={{ fontSize: 11, py: 0, minWidth: 0 }} disabled={busy} onClick={() => void generate()}>
            生成 intent
          </Button>
        </Box>
      </Box>
      {error && <Typography sx={{ fontSize: 11, color: '#ba1a1a' }}>{error}</Typography>}
      <Menu anchorEl={menu} open={Boolean(menu)} onClose={() => setMenu(null)}>
        <MenuItem onClick={() => void doIgnore(1)}>忽略 1 天</MenuItem>
        <MenuItem onClick={() => void doIgnore(7)}>忽略 7 天</MenuItem>
        <MenuItem onClick={() => void doIgnore(null)}>永久忽略</MenuItem>
      </Menu>
      {open && <SignalDetail s={s} onClose={() => setOpen(false)} />}
    </Paper>
  );
}
