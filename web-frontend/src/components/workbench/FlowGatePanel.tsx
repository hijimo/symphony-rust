import { useEffect, useState } from 'react';
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
  TextField,
  Tooltip,
  Typography,
} from '@mui/material';
import * as api from '../../api/changes';
import { useWorkbenchStore } from '../../store/workbenchStore';
import type { DocStatus, DocTemplate, DocType, FlowState, GateAction, ValidationIssue } from '../../types/changes';

export const STATUS_LABEL: Record<DocStatus, string> = {
  draft: '草稿',
  in_review: '待审',
  changes_requested: '要求修改',
  approved: '已批准',
  stale: '需重审',
  rejected: '已拒绝',
  merged: '已合入',
};

export const STATUS_COLOR: Record<DocStatus, 'default' | 'warning' | 'success' | 'error' | 'info'> = {
  draft: 'default',
  in_review: 'warning',
  changes_requested: 'error',
  approved: 'success',
  stale: 'warning',
  rejected: 'error',
  merged: 'info',
};

export const FLOW_LABEL: Record<FlowState, string> = {
  intent: '起草 intent',
  intent_review: '待审 intent（G1）',
  spec: '起草 spec',
  spec_review: '待审 spec（G2）',
  plan: '起草 plan',
  plan_review: '待审 plan（G3）',
  plan_approved_pending_merge: '已批准，待合并',
  todo: '文档已合入，进入实现',
  rejected: '已拒绝',
};

const GATE: Record<DocType, string> = { intent: 'G1', spec: 'G2', plan: 'G3' };

export default function FlowGatePanel() {
  const { flow, projectId, iid, stage, setStage, applyFlow, pushNotice, openFile, stages, context } = useWorkbenchStore();
  const [busy, setBusy] = useState(false);
  const [issues, setIssues] = useState<ValidationIssue[] | null>(null);
  const [templates, setTemplates] = useState<DocTemplate[]>([]);
  const [dialog, setDialog] = useState<null | 'request_changes' | 'reject' | 'revert'>(null);
  const [comment, setComment] = useState('');
  const [revertTo, setRevertTo] = useState<DocType>('plan');
  const [implBranch, setImplBranch] = useState('');

  // Validation results belong to one document state (verify B minor).
  const docKey = `${stage}:${flow?.documents.find((d) => d.docType === stage)?.status ?? ''}`;
  useEffect(() => setIssues(null), [docKey]);

  useEffect(() => {
    if (!projectId || !iid) return;
    api.listTemplates(projectId, iid).then(setTemplates).catch(() => setTemplates([]));
  }, [projectId, iid]);

  if (!flow) return null;
  const doc = flow.documents.find((d) => d.docType === stage);
  const checklist = templates.find((t) => t.docType === stage)?.checklist ?? [];
  const running = stages[stage].running;
  const precheck = flow.mergePrecheck;

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      const r = await fn();
      if (r && typeof r === 'object' && 'flow' in (r as object)) applyFlow(r as never);
      else await useWorkbenchStore.getState().refreshFlow();
    } catch (e) {
      pushNotice('error', (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const decide = (action: GateAction, c?: string) => run(() => api.decideDoc(projectId, iid, stage, action, c));

  const validateNow = async () => {
    try {
      setIssues(await api.validateDoc(projectId, iid, stage));
    } catch (e) {
      pushNotice('error', (e as Error).message);
    }
  };

  return (
    <Box sx={{ p: 1, display: 'flex', flexDirection: 'column', gap: 1, overflow: 'auto', height: '100%' }} data-testid="gate-panel">
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
        <Typography sx={{ fontWeight: 600, fontSize: 14 }}>#{flow.flow.issueIid}</Typography>
        <Chip size="small" label={FLOW_LABEL[flow.state]} color={flow.state === 'todo' ? 'info' : 'primary'} data-testid="flow-state" />
        <Chip size="small" variant="outlined" label={`track:${flow.flow.track}`} />
        {flow.flow.revision > 1 && <Chip size="small" variant="outlined" label={`r${flow.flow.revision}`} />}
        {flow.flow.docsMrUrl && (
          <Link href={flow.flow.docsMrUrl} target="_blank" rel="noreferrer" sx={{ fontSize: 12 }}>
            文档 MR
          </Link>
        )}
      </Box>
      {context?.behindMain && (
        <Alert severity="info" sx={{ py: 0 }} action={<Button size="small" onClick={() => run(() => api.mergeMain(projectId, iid))}>更新到最新主干</Button>}>
          主干有更新
        </Alert>
      )}
      {context && context.missing.length > 0 && (
        <Alert severity="warning" sx={{ py: 0 }}>上下文不完整：{context.missing.join('；')}</Alert>
      )}

      <Box sx={{ display: 'flex', gap: 0.5 }}>
        {flow.documents.map((d) => (
          <Chip
            key={d.docType}
            label={`${d.docType} · ${STATUS_LABEL[d.status]}`}
            color={STATUS_COLOR[d.status]}
            variant={d.docType === stage ? 'filled' : 'outlined'}
            onClick={() => {
              setStage(d.docType);
              openFile(`${flow.writableDir}/${d.docType}.md`);
            }}
            size="small"
            data-testid={`doc-chip-${d.docType}`}
          />
        ))}
      </Box>

      {doc?.draftState === 'running' && <Alert severity="info" sx={{ py: 0 }}>AI 正在起草 {stage}…</Alert>}
      {doc?.draftState === 'failed' && (
        <Alert
          severity="error"
          sx={{ py: 0 }}
          action={
            <>
              <Button size="small" onClick={() => run(() => api.retryDraft(projectId, iid, stage))}>重试</Button>
              <Button size="small" onClick={() => run(() => api.manualDraft(projectId, iid, stage))}>改为人工撰写</Button>
            </>
          }
        >
          起草失败（{doc.draftFailures} 次）：{doc.draftError}
        </Alert>
      )}
      {doc?.lastFeedback && doc.status !== 'approved' && (
        <Alert severity="warning" sx={{ py: 0 }}>审阅意见：{doc.lastFeedback}</Alert>
      )}
      {flow.flow.mergeError && flow.state === 'plan_approved_pending_merge' && (
        <Alert
          severity="error"
          sx={{ py: 0 }}
          action={<Button size="small" disabled={busy || !precheck?.canMerge} onClick={() => run(() => api.retryMerge(projectId, iid))}>重试合并</Button>}
        >
          已批准，待合并：{flow.flow.mergeError}
        </Alert>
      )}

      {doc && (
        <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap' }}>
          {['draft', 'changes_requested', 'stale'].includes(doc.status) && (
            <>
              <Button size="small" variant="outlined" onClick={validateNow}>校验</Button>
              <Button size="small" variant="contained" disabled={busy || running} onClick={() => run(() => api.submitDoc(projectId, iid, stage))}>
                提交审阅
              </Button>
            </>
          )}
          {doc.status === 'in_review' && (
            <>
              <Tooltip title={stage === 'plan' && precheck && !precheck.canMerge ? (precheck.reason ?? '') : ''}>
                <span>
                  <Button
                    size="small"
                    variant="contained"
                    color="success"
                    disabled={busy || running || (stage === 'plan' && precheck !== undefined && !precheck.canMerge)}
                    onClick={() => decide('approve')}
                  >
                    批准（{GATE[stage]}）
                  </Button>
                </span>
              </Tooltip>
              <Button size="small" variant="outlined" disabled={busy || running} onClick={() => setDialog('request_changes')}>
                要求修改
              </Button>
              {stage === 'intent' && (
                <Button size="small" color="error" disabled={busy || running} onClick={() => setDialog('reject')}>
                  拒绝
                </Button>
              )}
            </>
          )}
          {flow.state === 'todo' && (
            <Button size="small" color="warning" variant="outlined" onClick={() => setDialog('revert')}>
              退回文档阶段
            </Button>
          )}
        </Box>
      )}
      {stage === 'plan' && doc?.status === 'in_review' && precheck && !precheck.canMerge && (
        <Alert severity="warning" sx={{ py: 0 }}>{precheck.reason}</Alert>
      )}

      {issues && (
        issues.length === 0 ? (
          <Alert severity="success" sx={{ py: 0 }}>校验通过</Alert>
        ) : (
          <Alert severity="error" sx={{ py: 0 }} data-testid="validation-issues">
            {issues.map((i, k) => (
              <div key={k}>
                <b>{i.section}</b>：{i.message}
              </div>
            ))}
          </Alert>
        )
      )}

      {checklist.length > 0 && doc?.status === 'in_review' && (
        <Box>
          <Typography sx={{ fontSize: 12, fontWeight: 600, mb: 0.5 }}>审阅清单</Typography>
          {checklist.map((c) => (
            <Typography key={c} sx={{ fontSize: 12 }}>☐ {c}</Typography>
          ))}
        </Box>
      )}

      {flow.approvals.length > 0 && (
        <Box>
          <Typography sx={{ fontSize: 12, fontWeight: 600, mb: 0.5 }}>审批记录</Typography>
          {flow.approvals.map((a) => (
            <Typography key={a.id} sx={{ fontSize: 11, color: a.invalidatedAt ? '#9aa0b4' : '#434655', textDecoration: a.invalidatedAt ? 'line-through' : undefined }}>
              {a.gate} {a.action === 'approve' ? '批准' : a.action === 'reject' ? '拒绝' : '要求修改'} · {a.actorName} · {a.createdAt} · {a.blobSha.slice(0, 8)}
            </Typography>
          ))}
        </Box>
      )}

      <Dialog open={dialog === 'request_changes' || dialog === 'reject'} onClose={() => setDialog(null)} fullWidth>
        <DialogTitle>{dialog === 'reject' ? '拒绝 intent' : `要求修改 ${stage}`}</DialogTitle>
        <DialogContent>
          <TextField
            autoFocus
            fullWidth
            multiline
            minRows={3}
            label={dialog === 'reject' ? '理由（可选）' : '修改意见（必填，将作为 AI 下一轮输入）'}
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            sx={{ mt: 1 }}
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialog(null)}>取消</Button>
          <Button
            variant="contained"
            color={dialog === 'reject' ? 'error' : 'primary'}
            disabled={dialog === 'request_changes' && !comment.trim()}
            onClick={async () => {
              const action = dialog as GateAction;
              setDialog(null);
              await decide(action, comment.trim() || undefined);
              setComment('');
            }}
          >
            确定
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog open={dialog === 'revert'} onClose={() => setDialog(null)} fullWidth>
        <DialogTitle>退回文档阶段</DialogTitle>
        <DialogContent>
          <Typography sx={{ fontSize: 13, mb: 1 }}>
            会停止实现阶段的 agent，把代码 MR 转为 Draft，并开 r{flow.flow.revision + 1} 文档 MR 重新走闸门。
          </Typography>
          <TextField select fullWidth size="small" label="退回到" value={revertTo} onChange={(e) => setRevertTo(e.target.value as DocType)} sx={{ mb: 1 }}>
            {flow.documents.filter((d) => d.docType !== 'intent').map((d) => (
              <MenuItem key={d.docType} value={d.docType}>{d.docType} 审阅</MenuItem>
            ))}
          </TextField>
          <TextField fullWidth size="small" label="实现分支（可选，用于复制实现期修订）" value={implBranch} onChange={(e) => setImplBranch(e.target.value)} />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDialog(null)}>取消</Button>
          <Button
            variant="contained"
            color="warning"
            onClick={async () => {
              setDialog(null);
              await run(() => api.revertFlow(projectId, iid, { to: revertTo, implBranch: implBranch.trim() || undefined }));
              await useWorkbenchStore.getState().init(projectId, iid);
            }}
          >
            退回
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}
