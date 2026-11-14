import { useCallback, useEffect, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  MenuItem,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  TextField,
  Typography,
} from '@mui/material';
import { createSignalKey, listSignalKeys, revokeSignalKey } from '../../api/signals';
import type { CreatedSignalKey, SignalIngestKey, SignalSource } from '../../types/signals';

const SOURCES: SignalSource[] = ['generic', 'alertmanager', 'sentry'];

/** Ingest keys for external signals (P3 §3 S1). Creating/revoking needs project owner. */
export default function SignalKeysEditor({ projectId }: { projectId: number }) {
  const [keys, setKeys] = useState<SignalIngestKey[] | null>(null);
  const [name, setName] = useState('');
  const [source, setSource] = useState<SignalSource>('generic');
  const [created, setCreated] = useState<CreatedSignalKey | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setKeys(await listSignalKeys(projectId));
    } catch (e) {
      setError((e as Error).message);
    }
  }, [projectId]);

  useEffect(() => {
    void load();
  }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError('');
    try {
      await fn();
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (!keys && !error) return <CircularProgress size={24} />;

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Typography variant="body2" color="text.secondary">
        监控系统把告警推送到 <code>{`${window.location.origin}/api/ingest/${projectId}/<来源>`}</code>。
        generic 和 alertmanager 用请求头 <code>Authorization: Bearer &lt;密钥&gt;</code>；Sentry 把密钥填为内部集成的 Client Secret，
        按签名校验。同一指纹只记一条信号。
      </Typography>
      <Box sx={{ display: 'flex', gap: 1, alignItems: 'center' }}>
        <TextField
          size="small"
          label="密钥名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
          inputProps={{ maxLength: 100 }}
        />
        <TextField
          select
          size="small"
          label="来源"
          value={source}
          onChange={(e) => setSource(e.target.value as SignalSource)}
          sx={{ minWidth: 160 }}
        >
          {SOURCES.map((s) => (
            <MenuItem key={s} value={s}>
              {s}
            </MenuItem>
          ))}
        </TextField>
        <Button
          variant="contained"
          disabled={busy || !name.trim()}
          onClick={() =>
            void run(async () => {
              setCreated(await createSignalKey(projectId, { source, name: name.trim() }));
              setName('');
            })
          }
        >
          生成密钥
        </Button>
      </Box>
      {created && (
        <Alert severity="success" onClose={() => setCreated(null)}>
          {created.name}（{created.source}）的密钥只显示这一次，请立即保存：
          <Box component="code" sx={{ display: 'block', mt: 0.5, wordBreak: 'break-all' }}>
            {created.secret}
          </Box>
        </Alert>
      )}
      {error && <Alert severity="error">{error}</Alert>}
      {keys && keys.length > 0 && (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>名称</TableCell>
              <TableCell>来源</TableCell>
              <TableCell>创建时间</TableCell>
              <TableCell>最近使用</TableCell>
              <TableCell />
            </TableRow>
          </TableHead>
          <TableBody>
            {keys.map((k) => (
              <TableRow key={k.id} data-testid={`signal-key-${k.id}`}>
                <TableCell>{k.name}</TableCell>
                <TableCell>{k.source}</TableCell>
                <TableCell>{k.createdAt}</TableCell>
                <TableCell>{k.lastUsedAt ?? '—'}</TableCell>
                <TableCell align="right">
                  {k.revokedAt ? (
                    <Chip size="small" label="已吊销" />
                  ) : (
                    <Button
                      size="small"
                      color="error"
                      disabled={busy}
                      onClick={() => void run(() => revokeSignalKey(projectId, k.id))}
                    >
                      吊销
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Box>
  );
}
