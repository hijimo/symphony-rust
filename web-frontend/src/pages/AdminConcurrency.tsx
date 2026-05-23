import { Box, Typography } from '@mui/material';
import ConcurrencyPanel from '../components/concurrency/ConcurrencyPanel';

export default function AdminConcurrency() {
  return (
    <Box sx={{ p: 3, maxWidth: 900 }}>
      <Typography variant="h5" sx={{ fontWeight: 600, mb: 3 }}>
        并行控制
      </Typography>
      <ConcurrencyPanel />
    </Box>
  );
}
