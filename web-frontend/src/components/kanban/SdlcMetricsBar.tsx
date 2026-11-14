import { useEffect, useState } from 'react';
import { Box, Button, Collapse, Paper, Tooltip, Typography } from '@mui/material';
import { getSdlcMetrics } from '../../api/changes';
import type { SdlcMetrics } from '../../types/changes';

const OPEN_KEY = 'sdlcMetricsOpen';
const DASH = '—';

const pct = (rate: number | null, num?: number, den?: number): string =>
  rate === null || rate === undefined
    ? DASH
    : `${Math.round(rate * 100)}%${num !== undefined && den !== undefined ? `（${num}/${den}）` : ''}`;

function Metric({ id, label, value, hint }: { id: string; label: string; value: string; hint: string }) {
  return (
    <Tooltip title={hint}>
      <Box data-testid={`metric-${id}`} sx={{ minWidth: 120 }}>
        <Typography sx={{ fontSize: 11, color: '#737686' }}>{label}</Typography>
        <Typography sx={{ fontSize: 14, fontWeight: 600, color: '#191b23' }}>{value}</Typography>
      </Box>
    </Tooltip>
  );
}

/** Collapsible document-flow metrics above the project board (P2 §4 B5). */
export default function SdlcMetricsBar({ projectId }: { projectId: number }) {
  const [open, setOpen] = useState(() => localStorage.getItem(OPEN_KEY) !== '0');
  const [m, setM] = useState<SdlcMetrics | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    let alive = true;
    getSdlcMetrics(projectId)
      .then((v) => alive && setM(v))
      .catch((e: Error) => alive && setError(e.message));
    return () => {
      alive = false;
    };
  }, [projectId]);

  const toggle = () => {
    localStorage.setItem(OPEN_KEY, open ? '0' : '1');
    setOpen(!open);
  };
  const fp = m?.firstPassMerge;
  const pc = m?.planConformance;

  return (
    <Paper variant="outlined" sx={{ px: 2, py: 1, mb: 2, borderRadius: '8px' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <Typography sx={{ fontSize: 13, fontWeight: 600 }}>流程度量 · 近 {m?.days ?? 30} 天</Typography>
        <Button size="small" onClick={toggle} sx={{ fontSize: 12 }} aria-label={open ? '收起度量' : '展开度量'}>
          {open ? '收起' : '展开'}
        </Button>
      </Box>
      <Collapse in={open}>
        {error && <Typography sx={{ fontSize: 12, color: '#ba1a1a' }}>度量加载失败：{error}</Typography>}
        {m && (
          <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 3, py: 1 }}>
            <Metric
              id="intent"
              label="intent 存活率"
              value={pct(m.intentSurvival.rate, m.intentSurvival.accepted, m.intentSurvival.accepted + m.intentSurvival.rejected)}
              hint="G1 批准数 ÷（批准数 + 拒绝数）"
            />
            <Metric
              id="cycle"
              label="设计阶段周期"
              value={
                m.designCycle.medianHours === null
                  ? DASH
                  : `中位 ${m.designCycle.medianHours}h · P90 ${m.designCycle.p90Hours}h`
              }
              hint={`G1 批准到 G2 批准，样本 ${m.designCycle.count} 个`}
            />
            <Metric
              id="rework"
              label="需求返工"
              value={`${m.rework.count} 次 · ${m.rework.flows} 个变更`}
              hint="plan 首次提交审阅后，spec 又被修改或被级联置为 stale 的次数"
            />
            <Metric
              id="firstPass"
              label="首轮合并率"
              value={pct(fp?.rate ?? null, fp?.firstPass, fp?.closed)}
              hint={fp?.error ? `平台查询失败：${fp.error}` : `已关闭且未被拒绝的 issue 中从未带过 Rework 的比例${fp?.partial ? '（只统计了部分 issue）' : ''}`}
            />
            <Metric
              id="conformance"
              label="计划一致性"
              value={pct(pc?.rate ?? null, pc?.conforming, pc?.closed)}
              hint={pc?.error ? `平台查询失败：${pc.error}` : '已关闭的有 plan 的 issue 中，关闭时不带 plan-deviation 的比例'}
            />
            <Metric
              id="tokens"
              label="起草 token"
              value={m.draftTokens.total.toLocaleString('en-US')}
              hint="窗口内文档起草会话的 token 总量"
            />
          </Box>
        )}
      </Collapse>
    </Paper>
  );
}
