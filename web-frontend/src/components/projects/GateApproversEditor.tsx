import { useEffect, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Checkbox,
  CircularProgress,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Typography,
} from '@mui/material';
import { getMembers } from '../../api/members';
import { getGateApprovers, putGateApprovers } from '../../api/changes';
import { useAuthStore } from '../../store/auth';
import type { ProjectMember } from '../../types';
import type { GateApproverLists, GateKey } from '../../types/changes';

const GATES: { key: GateKey; doc: string }[] = [
  { key: 'G1', doc: 'intent' },
  { key: 'G2', doc: 'spec' },
  { key: 'G3', doc: 'plan' },
];

type Selection = Record<GateKey, number[]>;

const toSelection = (l: GateApproverLists): Selection => ({
  G1: l.G1.map((a) => a.userId),
  G2: l.G2.map((a) => a.userId),
  G3: l.G3.map((a) => a.userId),
});

const same = (a: Selection, b: Selection): boolean =>
  GATES.every(({ key }) => [...a[key]].sort().join() === [...b[key]].sort().join());

/** Who may approve each document gate (P2 §4 B6). Only owners edit. */
export default function GateApproversEditor({ projectId }: { projectId: number }) {
  const me = useAuthStore((s) => s.user);
  const [members, setMembers] = useState<ProjectMember[] | null>(null);
  const [saved, setSaved] = useState<Selection | null>(null);
  const [sel, setSel] = useState<Selection | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  useEffect(() => {
    let alive = true;
    Promise.all([getMembers(projectId), getGateApprovers(projectId)])
      .then(([m, l]) => {
        if (!alive) return;
        setMembers(m);
        setSaved(toSelection(l));
        setSel(toSelection(l));
      })
      .catch((e: Error) => alive && setError(e.message));
    return () => {
      alive = false;
    };
  }, [projectId]);

  if (error && !sel) return <Alert severity="error">{error}</Alert>;
  if (!members || !sel || !saved) return <CircularProgress size={24} />;

  const canEdit = me?.role === 'admin' || members.some((m) => m.user_id === me?.id && m.role === 'owner');
  const name = (id: number) => {
    const m = members.find((x) => x.user_id === id);
    return m ? (m.display_name ?? m.username) : `#${id}`;
  };
  const toggle = (gate: GateKey, id: number) => {
    setDone(false);
    const cur = sel[gate];
    setSel({ ...sel, [gate]: cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id] });
  };
  const save = async () => {
    setBusy(true);
    setError('');
    try {
      const l = toSelection(await putGateApprovers(projectId, sel));
      setSaved(l);
      setSel(l);
      setDone(true);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1.5 }}>
      <Typography variant="body2" color="text.secondary">
        某道闸门不选人时，所有项目成员都能审批；选了人后只有这些人（以及系统管理员）能批准、要求修改或拒绝。
      </Typography>
      {GATES.map(({ key, doc }) => (
        <Typography key={key} variant="body2">
          {key} · {doc}：{sel[key].length === 0 ? '所有成员可审批' : `仅 ${sel[key].map(name).join('、')}`}
        </Typography>
      ))}
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell>成员</TableCell>
            {GATES.map(({ key, doc }) => (
              <TableCell key={key} align="center">
                {key}（{doc}）
              </TableCell>
            ))}
          </TableRow>
        </TableHead>
        <TableBody>
          {members.map((m) => (
            <TableRow key={m.user_id}>
              <TableCell>{m.display_name ?? m.username}</TableCell>
              {GATES.map(({ key }) => (
                <TableCell key={key} align="center">
                  <Checkbox
                    size="small"
                    checked={sel[key].includes(m.user_id)}
                    disabled={!canEdit || busy}
                    onChange={() => toggle(key, m.user_id)}
                    inputProps={{ 'aria-label': `${m.display_name ?? m.username} 可审批 ${key}` }}
                  />
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {error && <Alert severity="error">{error}</Alert>}
      {done && <Alert severity="success">已保存</Alert>}
      {canEdit ? (
        <Box sx={{ display: 'flex', justifyContent: 'flex-end' }}>
          <Button variant="contained" disabled={busy || same(sel, saved)} onClick={() => void save()}>
            保存审批人
          </Button>
        </Box>
      ) : (
        <Typography variant="caption" color="text.secondary">
          只有项目 owner 可以修改审批人
        </Typography>
      )}
    </Box>
  );
}
