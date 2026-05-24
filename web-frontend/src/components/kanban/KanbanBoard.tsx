import { Alert, Box, Button, Typography } from '@mui/material';
import KanbanColumn from './KanbanColumn';
import IssueCard from './IssueCard';
import PrCard from './PrCard';
import type { KanbanData } from '../../types/kanban';

interface KanbanBoardProps {
  data: KanbanData;
  onLoadMore?: () => void;
  loadingMore?: boolean;
}

export default function KanbanBoard({ data, onLoadMore, loadingMore }: KanbanBoardProps) {
  const prColumnTitle = data.platform === 'github' ? 'PR' : 'MR';

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: { xs: '1fr', md: 'repeat(3, 1fr)' },
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

      {/* PR column */}
      <KanbanColumn
        title={prColumnTitle}
        count={data.pr.total_count}
        headerColor="#832600"
      >
        {data.pr.error ? (
          <Alert severity="error" sx={{ borderRadius: '8px', fontSize: '12px' }}>
            {data.pr.error}
          </Alert>
        ) : data.pr.merge_requests.length === 0 ? (
          <EmptyColumn message={`暂无待处理 ${prColumnTitle}`} />
        ) : (
          data.pr.merge_requests.map((mr) => (
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
