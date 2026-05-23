import { useState, useEffect, useCallback } from 'react';
import {
  Box,
  Paper,
  Typography,
  Button,
  TextField,
  Snackbar,
  Alert,
  CircularProgress,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Chip,
  IconButton,
  Tooltip,
} from '@mui/material';
import {
  Save,
  Refresh,
  Settings,
  TrendingUp,
} from '@mui/icons-material';
import {
  getSystemConfig,
  updateSystemConfig,
  getSystemStats,
} from '../api/adminConfig';
import type { SystemConfigItem, SystemStats } from '../api/adminConfig';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

interface EditableConfig extends SystemConfigItem {
  editValue: string;
  dirty: boolean;
}

export default function AdminConfig() {
  const [configs, setConfigs] = useState<EditableConfig[]>([]);
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [statsLoading, setStatsLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  const fetchConfigs = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getSystemConfig();
      setConfigs(
        data.map((c) => ({
          ...c,
          editValue: c.value,
          dirty: false,
        }))
      );
    } catch (err) {
      setSnack({ open: true, message: (err as Error).message, severity: 'error' });
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchStats = useCallback(async () => {
    setStatsLoading(true);
    try {
      const data = await getSystemStats();
      setStats(data);
    } catch (err) {
      setSnack({ open: true, message: (err as Error).message, severity: 'error' });
    } finally {
      setStatsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchConfigs();
    fetchStats();
  }, [fetchConfigs, fetchStats]);

  const handleValueChange = (key: string, newValue: string) => {
    setConfigs((prev) =>
      prev.map((c) =>
        c.key === key
          ? { ...c, editValue: newValue, dirty: newValue !== c.value }
          : c
      )
    );
  };

  const handleSave = async () => {
    const dirtyConfigs = configs.filter((c) => c.dirty);
    if (dirtyConfigs.length === 0) return;

    setSaving(true);
    try {
      const updated = await updateSystemConfig({
        configs: dirtyConfigs.map((c) => ({ key: c.key, value: c.editValue })),
      });
      setConfigs(
        updated.map((c) => ({
          ...c,
          editValue: c.value,
          dirty: false,
        }))
      );
      setSnack({ open: true, message: '配置已保存', severity: 'success' });
      // Refresh stats since config changes may affect them
      fetchStats();
    } catch (err) {
      setSnack({ open: true, message: (err as Error).message, severity: 'error' });
    } finally {
      setSaving(false);
    }
  };

  const hasDirty = configs.some((c) => c.dirty);

  return (
    <Box sx={{ p: 3, maxWidth: 1200, mx: 'auto' }}>
      {/* Header */}
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 3 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
          <Settings sx={{ fontSize: 28, color: 'text.secondary' }} />
          <Typography variant="h5" fontWeight={600}>
            系统配置
          </Typography>
        </Box>
        <Box sx={{ display: 'flex', gap: 1 }}>
          <Tooltip title="刷新">
            <IconButton onClick={() => { fetchConfigs(); fetchStats(); }} disabled={loading}>
              <Refresh />
            </IconButton>
          </Tooltip>
          <Button
            variant="contained"
            startIcon={saving ? <CircularProgress size={18} /> : <Save />}
            onClick={handleSave}
            disabled={!hasDirty || saving}
            sx={{
              background: hasDirty ? 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)' : undefined,
              borderRadius: '4px',
              textTransform: 'none',
            }}
          >
            保存修改
          </Button>
        </Box>
      </Box>

      {/* Stats Cards */}
      <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 2, mb: 3 }}>
        <StatCard
          label="项目总数"
          value={stats?.totalProjects}
          loading={statsLoading}
        />
        <StatCard
          label="运行中服务"
          value={stats?.runningServices}
          loading={statsLoading}
          color="#4caf50"
        />
        <StatCard
          label="用户总数"
          value={stats?.totalUsers}
          loading={statsLoading}
        />
        <StatCard
          label="并发使用"
          value={stats ? `${stats.globalConcurrencyUsed} / ${stats.globalConcurrencyLimit}` : undefined}
          loading={statsLoading}
          color="#ff9800"
        />
      </Box>

      {/* Config Table */}
      <Paper sx={{ borderRadius: '8px', overflow: 'hidden' }} elevation={0} variant="outlined">
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow sx={{ bgcolor: 'action.hover' }}>
                <TableCell sx={{ fontWeight: 600, width: '25%' }}>配置项</TableCell>
                <TableCell sx={{ fontWeight: 600, width: '30%' }}>值</TableCell>
                <TableCell sx={{ fontWeight: 600, width: '30%' }}>说明</TableCell>
                <TableCell sx={{ fontWeight: 600, width: '15%' }}>更新时间</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {loading ? (
                <TableRow>
                  <TableCell colSpan={4} align="center" sx={{ py: 4 }}>
                    <CircularProgress size={32} />
                  </TableCell>
                </TableRow>
              ) : configs.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={4} align="center" sx={{ py: 4, color: 'text.secondary' }}>
                    暂无配置项
                  </TableCell>
                </TableRow>
              ) : (
                configs.map((config) => (
                  <TableRow key={config.key} hover>
                    <TableCell>
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                        <Typography variant="body2" fontFamily="monospace" fontWeight={500}>
                          {config.key}
                        </Typography>
                        {config.dirty && (
                          <Chip label="已修改" size="small" color="warning" variant="outlined" />
                        )}
                      </Box>
                    </TableCell>
                    <TableCell>
                      <TextField
                        size="small"
                        value={config.editValue}
                        onChange={(e) => handleValueChange(config.key, e.target.value)}
                        variant="filled"
                        hiddenLabel
                        sx={{
                          width: '100%',
                          '& .MuiFilledInput-root': {
                            borderRadius: '4px',
                            bgcolor: config.dirty ? 'warning.50' : undefined,
                          },
                        }}
                        inputProps={{ style: { fontFamily: 'monospace' } }}
                      />
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2" color="text.secondary">
                        {config.description || '-'}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2" color="text.secondary">
                        {config.updatedAt}
                      </Typography>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </TableContainer>
      </Paper>

      {/* Snackbar */}
      <Snackbar
        open={snack.open}
        autoHideDuration={4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: 'top', horizontal: 'center' }}
      >
        <Alert
          severity={snack.severity}
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
          variant="filled"
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}

interface StatCardProps {
  label: string;
  value: number | string | undefined;
  loading: boolean;
  color?: string;
}

function StatCard({ label, value, loading, color }: StatCardProps) {
  return (
    <Paper
      sx={{ p: 2.5, borderRadius: '8px', display: 'flex', alignItems: 'center', gap: 2 }}
      elevation={0}
      variant="outlined"
    >
      <TrendingUp sx={{ color: color || 'primary.main', fontSize: 32 }} />
      <Box>
        <Typography variant="body2" color="text.secondary">
          {label}
        </Typography>
        {loading ? (
          <CircularProgress size={20} />
        ) : (
          <Typography variant="h6" fontWeight={600} sx={{ color: color || 'text.primary' }}>
            {value ?? '-'}
          </Typography>
        )}
      </Box>
    </Paper>
  );
}
