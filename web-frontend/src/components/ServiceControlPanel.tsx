import { Box, Typography, Button, Chip, CircularProgress, Switch, FormControlLabel, TextField } from '@mui/material';
import { PlayArrow, Stop, RestartAlt } from '@mui/icons-material';
import type { ServiceStatusData } from '../types';

interface ServiceControlPanelProps {
  status: ServiceStatusData | null;
  loading: boolean;
  actionLoading: string | null;
  autoRestart: boolean;
  maxConcurrentAgents: number;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onAutoRestartChange: (value: boolean) => void;
  onMaxAgentsChange: (value: number) => void;
  onSaveConfig: () => void;
  configSaving: boolean;
  configChanged: boolean;
}

function getStatusColor(status: string): 'success' | 'error' | 'warning' | 'default' | 'info' {
  switch (status) {
    case 'running': return 'success';
    case 'stopped': return 'default';
    case 'starting':
    case 'stopping': return 'info';
    case 'error':
    case 'failed': return 'error';
    default: return 'default';
  }
}

function getStatusLabel(status: string): string {
  switch (status) {
    case 'running': return '运行中';
    case 'stopped': return '已停止';
    case 'starting': return '启动中';
    case 'stopping': return '停止中';
    case 'error': return '异常';
    case 'failed': return '启动失败';
    default: return status;
  }
}

function formatUptime(seconds: number | null): string {
  if (seconds === null || seconds === undefined) return '-';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export default function ServiceControlPanel({
  status,
  loading,
  actionLoading,
  autoRestart,
  maxConcurrentAgents,
  onStart,
  onStop,
  onRestart,
  onAutoRestartChange,
  onMaxAgentsChange,
  onSaveConfig,
  configSaving,
  configChanged,
}: ServiceControlPanelProps) {
  if (loading || !status) {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'center', py: 4 }}>
        <CircularProgress size={32} />
      </Box>
    );
  }

  const isRunning = status.status === 'running';
  const isStopped = status.status === 'stopped' || status.status === 'error' || status.status === 'failed';
  const isTransitioning = status.status === 'starting' || status.status === 'stopping';

  return (
    <Box>
      {/* Status Badge */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, mb: 3 }}>
        <Typography variant="subtitle2" color="text.secondary">
          当前状态
        </Typography>
        <Chip
          label={getStatusLabel(status.status)}
          color={getStatusColor(status.status)}
          variant="filled"
          size="medium"
          sx={{ fontSize: '14px', fontWeight: 500, px: 1 }}
        />
      </Box>

      {/* Control Buttons */}
      <Box sx={{ display: 'flex', gap: 1.5, mb: 3 }}>
        <Button
          variant="contained"
          color="primary"
          startIcon={actionLoading === 'start' ? <CircularProgress size={16} color="inherit" /> : <PlayArrow />}
          onClick={onStart}
          disabled={!isStopped || !!actionLoading || isTransitioning}
        >
          启动
        </Button>
        <Button
          variant="outlined"
          color="error"
          startIcon={actionLoading === 'stop' ? <CircularProgress size={16} color="inherit" /> : <Stop />}
          onClick={onStop}
          disabled={!isRunning || !!actionLoading || isTransitioning}
        >
          停止
        </Button>
        <Button
          variant="outlined"
          color="inherit"
          startIcon={actionLoading === 'restart' ? <CircularProgress size={16} color="inherit" /> : <RestartAlt />}
          onClick={onRestart}
          disabled={!isRunning || !!actionLoading || isTransitioning}
        >
          重启
        </Button>
      </Box>

      {/* Service Info */}
      <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))', gap: 2, mb: 3 }}>
        <Box>
          <Typography variant="body2" color="text.secondary">PID</Typography>
          <Typography variant="subtitle2">{status.pid ?? '-'}</Typography>
        </Box>
        <Box>
          <Typography variant="body2" color="text.secondary">运行时长</Typography>
          <Typography variant="subtitle2">{formatUptime(status.uptime_seconds)}</Typography>
        </Box>
        <Box>
          <Typography variant="body2" color="text.secondary">重启次数</Typography>
          <Typography variant="subtitle2">{status.restart_count}</Typography>
        </Box>
        <Box>
          <Typography variant="body2" color="text.secondary">启动时间</Typography>
          <Typography variant="subtitle2">
            {status.started_at ? new Date(status.started_at).toLocaleString('zh-CN') : '-'}
          </Typography>
        </Box>
      </Box>

      {/* Error Message */}
      {status.error_message && (
        <Box sx={{ mb: 3, p: 2, bgcolor: '#ffdad6', borderRadius: '4px' }}>
          <Typography variant="body2" color="error.dark" sx={{ fontFamily: 'monospace', whiteSpace: 'pre-wrap' }}>
            {status.error_message}
          </Typography>
        </Box>
      )}

      {/* Config Section */}
      <Box sx={{ borderTop: '1px solid #c3c6d7', pt: 3 }}>
        <Typography variant="subtitle2" color="text.primary" sx={{ mb: 2 }}>
          服务配置
        </Typography>
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, maxWidth: 400 }}>
          <FormControlLabel
            control={
              <Switch
                checked={autoRestart}
                onChange={(e) => onAutoRestartChange(e.target.checked)}
              />
            }
            label="崩溃后自动重启（最多 3 次）"
          />
          <TextField
            label="最大并发 Agent 数"
            type="number"
            value={maxConcurrentAgents}
            onChange={(e) => {
              const val = parseInt(e.target.value, 10);
              if (val >= 1 && val <= 20) onMaxAgentsChange(val);
            }}
            slotProps={{ htmlInput: { min: 1, max: 20 } }}
            size="small"
            sx={{ maxWidth: 200 }}
            helperText="范围 1-20"
          />
        </Box>
        <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 2 }}>
          <Button
            variant="contained"
            onClick={onSaveConfig}
            disabled={!configChanged || configSaving}
            startIcon={configSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
          >
            保存配置
          </Button>
        </Box>
      </Box>
    </Box>
  );
}
