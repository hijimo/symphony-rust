import { Box, Skeleton } from '@mui/material';

/** Three-column skeleton for kanban loading state */
export default function KanbanSkeleton() {
  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: { xs: '1fr', md: 'repeat(3, 1fr)' },
        gap: 2,
        minHeight: 400,
      }}
    >
      {[0, 1, 2].map((col) => (
        <Box key={col} sx={{ display: 'flex', flexDirection: 'column', gap: 1.5 }}>
          <Skeleton variant="rounded" height={40} sx={{ borderRadius: '8px' }} />
          {Array.from({ length: col === 0 ? 4 : col === 1 ? 2 : 1 }).map((_, i) => (
            <Skeleton
              key={i}
              variant="rounded"
              height={100}
              sx={{ borderRadius: '8px' }}
            />
          ))}
        </Box>
      ))}
    </Box>
  );
}
