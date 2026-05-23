import { useEffect, useState } from 'react';
import {
  Box,
  Typography,
  LinearProgress,
  IconButton,
  Alert,
  Chip,
} from '@mui/material';
import EditIcon from '@mui/icons-material/Edit';
import { useConcurrencyStore } from '../../store/concurrencyStore';
import ConcurrencyConfigDialog from './ConcurrencyConfigDialog';
import ProjectConcurrencyCard from './ProjectConcurrencyCard';

export default function ConcurrencyPanel() {
  const {
    globalMax,
    globalActive,
    utilizationPercent,
    projects,
    dataFreshnessSeconds,
    loading,
    error,
    sseConnected,
    fetchStatus,
    connectSSE,
    disconnectSSE,
  } = useConcurrencyStore();

  const [configOpen, setConfigOpen] = useState(false);

  useEffect(() => {
    fetchStatus();
    connectSSE();
    return () => disconnectSSE();
  }, [fetchStatus, connectSSE, disconnectSSE]);

  if (loading && globalMax === 0) {
    return (
      <Box sx={{ p: 3 }}>
        <LinearProgress />
      </Box>
    );
  }

  const progressColor =
    utilizationPercent >= 90
      ? 'error'
      : utilizationPercent >= 70
        ? 'warning'
        : 'primary';

  return (
    <Box>
      {error && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {error}
        </Alert>
      )}

      {dataFreshnessSeconds > 10 && (
        <Alert severity="warning" sx={{ mb: 2 }}>
          数据可能过期（最后更新于 {dataFreshnessSeconds} 秒前）
        </Alert>
      )}

      {/* Global Status */}
      <Box
        sx={{
          p: 3,
          bgcolor: '#f8f8fc',
          borderRadius: 2,
          mb: 3,
        }}
      >
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            mb: 2,
          }}
        >
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Typography variant="h6" sx={{ fontWeight: 500 }}>
              全局并行状态
            </Typography>
            <Chip
              label={sseConnected ? '实时' : '离线'}
              size="small"
              color={sseConnected ? 'success' : 'default'}
              variant="outlined"
            />
          </Box>
          <IconButton
            onClick={() => setConfigOpen(true)}
            aria-label="编辑配置"
            size="small"
          >
            <EditIcon />
          </IconButton>
        </Box>

        <Box sx={{ display: 'flex', alignItems: 'baseline', gap: 1, mb: 1 }}>
          <Typography variant="h4" sx={{ fontWeight: 600 }}>
            {globalActive} / {globalMax}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            活跃 Agent
          </Typography>
        </Box>

        <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
          <LinearProgress
            variant="determinate"
            value={Math.min(utilizationPercent, 100)}
            color={progressColor}
            sx={{ flex: 1, height: 8, borderRadius: 4 }}
            aria-valuenow={Math.round(utilizationPercent)}
            aria-label="并行利用率"
          />
          <Typography variant="body2" sx={{ fontWeight: 500, minWidth: 40 }}>
            {Math.round(utilizationPercent)}%
          </Typography>
        </Box>
      </Box>

      {/* Per-project breakdown */}
      <Typography variant="subtitle1" sx={{ fontWeight: 500, mb: 2 }}>
        项目并行详情
      </Typography>

      {projects.length === 0 ? (
        <Typography color="text.secondary">暂无运行中的项目</Typography>
      ) : (
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {projects.map((project) => (
            <ProjectConcurrencyCard key={project.project_id} project={project} />
          ))}
        </Box>
      )}

      <ConcurrencyConfigDialog
        open={configOpen}
        onClose={() => setConfigOpen(false)}
        currentMax={globalMax}
      />
    </Box>
  );
}
