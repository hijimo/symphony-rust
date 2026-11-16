import { useEffect, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from '@mui/material';
import { getOrgConfig, syncOrgConfig } from '../api/orgConfig';
import { updateSystemConfig } from '../api/adminConfig';
import type { OrgConfigStatus } from '../types/orgConfig';

const short = (sha: string | null) => (sha ? sha.slice(0, 8) : '—');

/** Administrator page of the organization rules repository (P3 §4 O4). */
export default function AdminOrgConfig() {
  const [status, setStatus] = useState<OrgConfigStatus | null>(null);
  const [url, setUrl] = useState('');
  const [ref, setRef] = useState('main');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const apply = (s: OrgConfigStatus) => {
    setStatus(s);
    setUrl(s.url ?? '');
    setRef(s.ref);
  };

  useEffect(() => {
    getOrgConfig()
      .then(apply)
      .catch((e: Error) => setError(e.message));
  }, []);

  const run = async (f: () => Promise<OrgConfigStatus>) => {
    setBusy(true);
    setError('');
    try {
      apply(await f());
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      await updateSystemConfig({
        configs: [
          { key: 'org_config.url', value: url.trim() || '-' },
          { key: 'org_config.ref', value: ref.trim() || 'main' },
        ],
      });
      return getOrgConfig();
    });

  if (!status) {
    return error ? <Alert severity="error">{error}</Alert> : <CircularProgress />;
  }

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
      <Typography variant="h5" sx={{ fontWeight: 600 }}>
        组织规则
      </Typography>
      <Card>
        <CardContent sx={{ display: 'flex', flexDirection: 'column', gap: 2, maxWidth: 640 }}>
          {!status.enabled && <Alert severity="info">未启用：填写仓库地址后保存即可启用</Alert>}
          <TextField
            label="仓库地址"
            size="small"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            helperText="https://、ssh://、git@ 或本机绝对路径。只读凭据通过环境变量 SYMPHONY_ORG_CONFIG_TOKEN 提供"
          />
          <TextField label="跟随的分支或 tag" size="small" value={ref} onChange={(e) => setRef(e.target.value)} />
          <Box sx={{ display: 'flex', gap: 1 }}>
            <Button variant="contained" onClick={save} disabled={busy}>
              保存
            </Button>
            <Button variant="outlined" onClick={() => run(syncOrgConfig)} disabled={busy || !status.enabled}>
              立即同步
            </Button>
            {busy && <CircularProgress size={20} />}
          </Box>
          {error && <Alert severity="error">{error}</Alert>}
        </CardContent>
      </Card>

      <Card>
        <CardContent sx={{ display: 'flex', flexDirection: 'column', gap: 1.5 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
            同步状态
          </Typography>
          <Typography variant="body2" data-testid="org-current">
            当前版本：{short(status.currentSha)}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            最近同步：{status.lastSyncAt ? new Date(status.lastSyncAt).toLocaleString() : '—'}（每 {status.intervalSecs} 秒）
          </Typography>
          {status.lastError && <Alert severity="error">{status.lastError}</Alert>}
          {status.droppedConfig.length > 0 && (
            <Box>
              <Typography variant="body2" sx={{ mb: 0.5 }}>
                config.toml 中被丢弃的配置项（只保留模型相关配置）：
              </Typography>
              <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap' }}>
                {status.droppedConfig.map((k) => (
                  <Chip key={k} size="small" variant="outlined" label={k} />
                ))}
              </Box>
            </Box>
          )}
          {status.pins.length > 0 && (
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell>项目</TableCell>
                  <TableCell>锁定</TableCell>
                  <TableCell>版本</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {status.pins.map((p) => (
                  <TableRow key={p.projectId}>
                    <TableCell>项目 #{p.projectId}</TableCell>
                    <TableCell>{p.pin}</TableCell>
                    <TableCell>{p.error ?? short(p.sha)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </Box>
  );
}
