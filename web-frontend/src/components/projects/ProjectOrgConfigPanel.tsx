import { useEffect, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  FormControlLabel,
  Radio,
  RadioGroup,
  TextField,
  Typography,
} from '@mui/material';
import { getProjectOrgConfig, setProjectOrgPin } from '../../api/orgConfig';
import type { ProjectOrgConfig } from '../../types/orgConfig';

const short = (sha: string | null) => (sha ? sha.slice(0, 8) : '—');

/** Organization rules version of the project (P3 §4 O4). Only owners edit. */
export default function ProjectOrgConfigPanel({ projectId }: { projectId: number }) {
  const [cfg, setCfg] = useState<ProjectOrgConfig | null>(null);
  const [mode, setMode] = useState<'follow' | 'pin'>('follow');
  const [pin, setPin] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const apply = (c: ProjectOrgConfig) => {
    setCfg(c);
    setMode(c.pin ? 'pin' : 'follow');
    setPin(c.pin ?? '');
  };

  useEffect(() => {
    let alive = true;
    getProjectOrgConfig(projectId)
      .then((c) => alive && apply(c))
      .catch((e: Error) => alive && setError(e.message));
    return () => {
      alive = false;
    };
  }, [projectId]);

  if (!cfg) {
    return error ? <Alert severity="error">{error}</Alert> : <CircularProgress size={20} />;
  }
  if (!cfg.enabled) {
    return <Typography color="text.secondary">未启用组织规则仓库</Typography>;
  }

  const save = async () => {
    setBusy(true);
    setError('');
    try {
      apply(await setProjectOrgPin(projectId, mode === 'pin' ? pin.trim() : null));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const disabled = !cfg.canEdit || busy;
  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1.5, maxWidth: 500 }}>
      <Typography variant="body2" color="text.secondary">
        来源仓库
      </Typography>
      <Typography variant="subtitle2" sx={{ fontFamily: 'monospace', wordBreak: 'break-all' }}>
        {cfg.url}
      </Typography>
      <Typography variant="body2" data-testid="org-effective">
        当前生效版本：{short(cfg.effectiveSha)}
        {cfg.pin ? `（锁定 ${cfg.pin}）` : `（跟随 ${cfg.ref}）`}
      </Typography>
      {cfg.pinError && <Alert severity="warning">{cfg.pinError}</Alert>}
      <RadioGroup row value={mode} onChange={(_, v) => setMode(v as 'follow' | 'pin')}>
        <FormControlLabel value="follow" control={<Radio size="small" />} label="跟随最新" disabled={disabled} />
        <FormControlLabel value="pin" control={<Radio size="small" />} label="锁定到 tag / commit" disabled={disabled} />
      </RadioGroup>
      {mode === 'pin' && (
        <TextField
          size="small"
          label="锁定版本"
          placeholder="v1.2.0 或 commit sha"
          value={pin}
          onChange={(e) => setPin(e.target.value)}
          disabled={disabled}
        />
      )}
      <Typography variant="caption" color="text.secondary">
        修改后，文档起草在下次启动会话时生效；编排器需重启服务后生效。
      </Typography>
      {!cfg.canEdit && (
        <Typography variant="caption" color="text.secondary">
          只有项目 owner 可以修改
        </Typography>
      )}
      {error && <Alert severity="error">{error}</Alert>}
      <Box>
        <Button
          variant="outlined"
          onClick={save}
          disabled={disabled || (mode === 'pin' && !pin.trim())}
          startIcon={busy ? <CircularProgress size={16} color="inherit" /> : undefined}
        >
          保存
        </Button>
      </Box>
    </Box>
  );
}
