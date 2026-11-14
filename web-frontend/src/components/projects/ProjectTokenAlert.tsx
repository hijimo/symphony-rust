import { useEffect, useState } from 'react';
import { Alert } from '@mui/material';
import { getProjectTokenStatus } from '../../api/changes';

/**
 * Warns when the project token (the creator's personal platform token) can
 * no longer be used; document MRs, labels and comments depend on it.
 */
export default function ProjectTokenAlert({ projectId }: { projectId: number }) {
  const [reason, setReason] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setReason(null);
    getProjectTokenStatus(projectId)
      .then((s) => {
        if (!cancelled && !s.ok) setReason(s.reason ?? '项目 Token 不可用');
      })
      .catch(() => {
        // A failed check is not evidence of a broken token.
      });
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  if (!reason) return null;
  return (
    <Alert severity="error" sx={{ mb: 2 }}>
      {reason}。文档流程的提交、MR、标签和评论都依赖这个 Token，请让项目创建者在个人设置中更新。
    </Alert>
  );
}
