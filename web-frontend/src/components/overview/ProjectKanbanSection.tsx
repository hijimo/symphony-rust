import { Box, Typography, Chip, Skeleton, Alert, Button } from '@mui/material';
import { useNavigate } from 'react-router-dom';
import OpenInNewIcon from '@mui/icons-material/OpenInNew';
import KanbanColumn from '../kanban/KanbanColumn';
import IssueCard from '../kanban/IssueCard';
import PrCard from '../kanban/PrCard';
import type { ProjectIssuesEntry, ProjectPrsEntry, ProjectMeta } from '../../types/overview';
import { getPendingMergeRequests } from '../../utils/kanbanPrs';

interface ProjectKanbanSectionProps {
  meta: ProjectMeta;
  issuesData?: ProjectIssuesEntry;
  prsData?: ProjectPrsEntry;
  prsLoading?: boolean;
}

export default function ProjectKanbanSection({
  meta,
  issuesData,
  prsData,
  prsLoading,
}: ProjectKanbanSectionProps) {
  const navigate = useNavigate();
  const prColumnTitle = meta.platform === 'github' ? 'PR' : 'MR';
  const pendingMrs = prsData ? getPendingMergeRequests(prsData.pr.merge_requests) : [];

  const hasProjectError = issuesData?.error;

  return (
    <Box
      sx={{
        border: '1px solid #c3c6d7',
        borderRadius: '8px',
        p: 2.5,
        bgcolor: '#fff',
      }}
    >
      {/* Project header */}
      <Box
        sx={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          mb: 2,
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, minWidth: 0 }}>
          <Typography
            variant="subtitle1"
            sx={{ fontWeight: 600, fontSize: '14px', color: '#191b23' }}
            noWrap
          >
            {meta.project_name}
          </Typography>
          <Chip
            label={meta.platform === 'github' ? 'GitHub' : 'GitLab'}
            size="small"
            sx={{
              height: 20,
              fontSize: '11px',
              fontWeight: 500,
              bgcolor: meta.platform === 'github' ? '#f0f0f0' : '#fef0e6',
              color: meta.platform === 'github' ? '#24292f' : '#832600',
              borderRadius: '4px',
            }}
          />
        </Box>
        <Button
          size="small"
          endIcon={<OpenInNewIcon sx={{ fontSize: '14px !important' }} />}
          onClick={() => navigate(`/projects/${meta.project_id}/kanban`)}
          sx={{
            textTransform: 'none',
            fontSize: '12px',
            color: '#434655',
            fontWeight: 500,
            '&:hover': { bgcolor: '#e7e7f3' },
          }}
        >
          查看详情
        </Button>
      </Box>

      {/* Project-level error */}
      {hasProjectError ? (
        <Alert
          severity="warning"
          sx={{ borderRadius: '8px', fontSize: '12px' }}
          action={
            <Button
              size="small"
              color="inherit"
              onClick={() => navigate(`/projects/${meta.project_id}/kanban`)}
              sx={{ fontSize: '12px' }}
            >
              前往查看
            </Button>
          }
        >
          {issuesData.error === 'no_token'
            ? '缺少平台 Token，请在个人设置中配置'
            : issuesData.error === 'timeout'
              ? '数据加载超时'
              : `加载失败: ${issuesData.error}`}
        </Alert>
      ) : (
        <Box
          sx={{
            display: 'grid',
            gridTemplateColumns: { xs: '1fr', md: `repeat(${issuesData?.testing ? 4 : 3}, 1fr)` },
            gap: 1.5,
            alignItems: 'start',
          }}
        >
          {/* Todo */}
          <KanbanColumn
            title="待处理"
            count={issuesData?.todo.total_count ?? 0}
            headerColor="#737686"
          >
            {!issuesData ? (
              <ColumnSkeleton />
            ) : issuesData.todo.issues.length === 0 ? (
              <EmptyHint message="暂无" />
            ) : (
              issuesData.todo.issues.map((issue) => (
                <IssueCard key={issue.iid} issue={issue} />
              ))
            )}
          </KanbanColumn>

          {/* In Progress */}
          <KanbanColumn
            title="处理中"
            count={issuesData?.in_progress.total_count ?? 0}
            headerColor="#0053db"
          >
            {!issuesData ? (
              <ColumnSkeleton />
            ) : issuesData.in_progress.issues.length === 0 ? (
              <EmptyHint message="暂无" />
            ) : (
              issuesData.in_progress.issues.map((issue) => (
                <IssueCard key={issue.iid} issue={issue} />
              ))
            )}
          </KanbanColumn>

          {/* Testing (conditional) */}
          {issuesData?.testing && (
            <KanbanColumn
              title="测试中"
              count={issuesData.testing.total_count}
              headerColor="#b45309"
            >
              {issuesData.testing.issues.length === 0 ? (
                <EmptyHint message="暂无" />
              ) : (
                issuesData.testing.issues.map((issue) => (
                  <IssueCard key={issue.iid} issue={issue} />
                ))
              )}
            </KanbanColumn>
          )}

          {/* PR/MR */}
          <KanbanColumn
            title={prColumnTitle}
            count={pendingMrs.length}
            headerColor="#832600"
          >
            {prsLoading || !prsData ? (
              <ColumnSkeleton />
            ) : prsData.error ? (
              <Alert severity="error" sx={{ borderRadius: '8px', fontSize: '11px' }}>
                加载失败
              </Alert>
            ) : pendingMrs.length === 0 ? (
              <EmptyHint message="暂无" />
            ) : (
              pendingMrs.map((mr) => <PrCard key={mr.iid} mr={mr} />)
            )}
          </KanbanColumn>
        </Box>
      )}
    </Box>
  );
}

function EmptyHint({ message }: { message: string }) {
  return (
    <Typography
      variant="body2"
      sx={{ color: '#737686', fontSize: '12px', textAlign: 'center', py: 2 }}
    >
      {message}
    </Typography>
  );
}

function ColumnSkeleton() {
  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
      <Skeleton variant="rounded" height={60} sx={{ borderRadius: '8px' }} />
      <Skeleton variant="rounded" height={60} sx={{ borderRadius: '8px' }} />
    </Box>
  );
}
