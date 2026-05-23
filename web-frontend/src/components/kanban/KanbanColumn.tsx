import { Box, Typography, Chip } from '@mui/material';
import type { ReactNode } from 'react';

interface KanbanColumnProps {
  title: string;
  count: number;
  children: ReactNode;
  headerColor?: string;
}

export default function KanbanColumn({
  title,
  count,
  children,
  headerColor,
}: KanbanColumnProps) {
  return (
    <Box
      sx={{
        display: 'flex',
        flexDirection: 'column',
        minHeight: 200,
        maxHeight: { xs: 'none', md: 'calc(100vh - 220px)' },
      }}
    >
      {/* Column header */}
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 1,
          px: 1.5,
          py: 1,
          mb: 1,
        }}
      >
        {headerColor && (
          <Box
            sx={{
              width: 4,
              height: 16,
              borderRadius: '2px',
              bgcolor: headerColor,
              flexShrink: 0,
            }}
          />
        )}
        <Typography
          sx={{
            fontSize: '14px',
            fontWeight: 500,
            color: '#191b23',
          }}
        >
          {title}
        </Typography>
        <Chip
          label={count}
          size="small"
          sx={{
            height: 20,
            minWidth: 24,
            fontSize: '11px',
            fontWeight: 500,
            bgcolor: '#e7e7f3',
            color: '#434655',
            borderRadius: '10px',
            '& .MuiChip-label': { px: 0.75 },
          }}
        />
      </Box>

      {/* Card list */}
      <Box
        sx={{
          display: 'flex',
          flexDirection: 'column',
          gap: 1,
          overflowY: 'auto',
          flex: 1,
          px: 0.5,
          pb: 1,
          '&::-webkit-scrollbar': {
            width: 4,
          },
          '&::-webkit-scrollbar-thumb': {
            bgcolor: '#c3c6d7',
            borderRadius: 2,
          },
        }}
      >
        {children}
      </Box>
    </Box>
  );
}
