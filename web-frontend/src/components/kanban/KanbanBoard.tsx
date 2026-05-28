import { Alert, Box, Button, Skeleton, Typography } from '@mui/material';
import KanbanColumn from './KanbanColumn';
import IssueCard from './IssueCard';
import PrCard from './PrCard';
import type { KanbanData } from '../../types/kanban';
import { getPendingMergeRequests } from '../../utils/kanbanPrs';

interface KanbanBoardProps {
  data: KanbanData;
  onLoadMore?: () => void;
  loadingMore?: boolean;
  prsLoading?: boolean;
}

export default function KanbanBoard({ data, onLoadMore, loadingMore, prsLoading }: KanbanBoardProps) {
  const prColumnTitle = data.platform === 'github' ? 'PR' : 'MR';
  const pendingMergeRequests = getPendingMergeRequests(data.pr.merge_requests);
  const hasTesting = !!data.testing;
  const columnCount = hasTesting ? 4 : 3;

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: { xs: '1fr', md: `repeat(${columnCount}, 1fr)` },
        gap: 2,
        alignItems: 'start',
      }}
    >
      {/* Todo column */}
      <KanbanColumn
        title="待处理"
        count={data.todo.total_count}
        headerColor="#737686"
      >
        {data.todo.issues.length === 0 ? (
          <EmptyColumn message="暂无待处理 Issue" />
        ) : (
          <>
            {data.todo.issues.map((issue) => (
              <IssueCard key={issue.iid} issue={issue} />
            ))}
            {data.todo.has_more && (
              <Button
                size="small"
                onClick={onLoadMore}
                disabled={loadingMore}
                sx={{
                  mt: 1,
                  color: '#434655',
                  bgcolor: '#e7e7f3',
                  borderRadius: '4px',
                  fontSize: '12px',
                  fontWeight: 500,
                  '&:hover': { bgcolor: '#d9d9e5' },
                }}
              >
                {loadingMore ? '加载中...' : '加载更多'}
              </Button>
            )}
          </>
        )}
      </KanbanColumn>

      {/* In Progress column */}
      <KanbanColumn
        title="处理中"
        count={data.in_progress.total_count}
        headerColor="#0053db"
      >
        {data.in_progress.issues.length === 0 ? (
          <EmptyColumn message="暂无处理中 Issue" />
        ) : (
          data.in_progress.issues.map((issue) => (
            <IssueCard key={issue.iid} issue={issue} />
          ))
        )}
      </KanbanColumn>

      {/* Testing column (conditional) */}
      {hasTesting && (
        <KanbanColumn
          title="测试中"
          count={data.testing!.total_count}
          headerColor="#b45309"
        >
          {data.testing!.issues.length === 0 ? (
            <EmptyColumn message="暂无测试中 Issue" />
          ) : (
            data.testing!.issues.map((issue) => (
              <IssueCard key={issue.iid} issue={issue} />
            ))
          )}
        </KanbanColumn>
      )}

      {/* PR column */}
      <KanbanColumn
        title={prColumnTitle}
        count={prsLoading ? 0 : pendingMergeRequests.length}
        headerColor="#832600"
      >
        {prsLoading ? (
          <PrColumnSkeleton />
        ) : data.pr.error ? (
          <Alert severity="error" sx={{ borderRadius: '8px', fontSize: '12px' }}>
            {data.pr.error}
          </Alert>
        ) : pendingMergeRequests.length === 0 ? (
          <EmptyColumn message={`暂无待处理 ${prColumnTitle}`} />
        ) : (
          pendingMergeRequests.map((mr) => (
            <PrCard key={mr.iid} mr={mr} />
          ))
        )}
      </KanbanColumn>
    </Box>
  );
}

function EmptyColumn({ message }: { message: string }) {
  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        py: 4,
      }}
    >
      <Typography
        variant="body2"
        sx={{ color: '#737686', fontSize: '12px' }}
      >
        {message}
      </Typography>
    </Box>
  );
}

function PrColumnSkeleton() {
  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1.5 }}>
      {[1, 2, 3].map((i) => (
        <Skeleton
          key={i}
          variant="rounded"
          height={80}
          sx={{ borderRadius: '8px' }}
        />
      ))}
    </Box>
  );
}
