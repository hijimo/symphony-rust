import { useState, useEffect, useCallback } from 'react';
import {
  Box,
  Typography,
  Tabs,
  Tab,
  Button,
  Snackbar,
  Alert,
  MenuItem,
  TextField,
  CircularProgress,
  Skeleton,
} from '@mui/material';
import SaveIcon from '@mui/icons-material/Save';
import { useAlertStore } from '../store/alertStore';
import AlertHistoryTable from '../components/alerts/AlertHistoryTable';
import AlertRuleCard from '../components/alerts/AlertRuleCard';
import ChannelConfigCard from '../components/alerts/ChannelConfigCard';
import type {
  AlertRule,
  AlertHistoryQuery,
  Severity,
  NotificationChannel,
} from '../types/alert';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

interface TabPanelProps {
  children: React.ReactNode;
  value: number;
  index: number;
}

function TabPanel({ children, value, index }: TabPanelProps) {
  if (value !== index) return null;
  return (
    <Box role="tabpanel" sx={{ pt: 3 }}>
      {children}
    </Box>
  );
}

export default function AdminAlerts() {
  const [tab, setTab] = useState(0);
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  // History filters
  const [historyQuery, setHistoryQuery] = useState<AlertHistoryQuery>({
    page_no: 1,
    page_size: 20,
  });
  const [severityFilter, setSeverityFilter] = useState<string>('');
  const [projectFilter, setProjectFilter] = useState<string>('');

  // Local editable rules state
  const [editedRules, setEditedRules] = useState<AlertRule[]>([]);
  const [rulesSaving, setRulesSaving] = useState(false);

  // Local editable channels state
  const [editedChannels, setEditedChannels] = useState<NotificationChannel[]>([]);
  const [channelsSaving, setChannelsSaving] = useState(false);

  const {
    rules,
    rulesLoading,
    rulesError,
    channels,
    availableTypes,
    channelsLoading,
    channelsError,
    history,
    historyLoading,
    historyError,
    fetchRules,
    fetchChannels,
    fetchHistory,
    updateRules,
    updateChannels,
    testNotification,
  } = useAlertStore();

  // Sync store rules to local editable state
  useEffect(() => {
    if (rules.length > 0) {
      setEditedRules(rules.map((r) => ({ ...r, threshold: { ...r.threshold } })));
    }
  }, [rules]);

  // Sync store channels to local editable state
  useEffect(() => {
    if (channels.length > 0) {
      setEditedChannels(channels.map((c) => ({ ...c, severityFilter: [...c.severityFilter] })));
    }
  }, [channels]);

  // Fetch data on tab change
  useEffect(() => {
    if (tab === 0) {
      fetchHistory(historyQuery);
    } else if (tab === 1) {
      fetchRules();
    } else if (tab === 2) {
      fetchChannels();
    }
  }, [tab]); // eslint-disable-line react-hooks/exhaustive-deps

  // Refetch history when query changes
  useEffect(() => {
    if (tab === 0) {
      fetchHistory(historyQuery);
    }
  }, [historyQuery]); // eslint-disable-line react-hooks/exhaustive-deps

  const handlePageChange = useCallback((page: number, pageSize: number) => {
    setHistoryQuery((prev) => ({ ...prev, page_no: page, page_size: pageSize }));
  }, []);

  const handleSeverityFilterChange = (value: string) => {
    setSeverityFilter(value);
    setHistoryQuery((prev) => ({
      ...prev,
      page_no: 1,
      severity: value ? (value as Severity) : undefined,
    }));
  };

  const handleProjectFilterChange = (value: string) => {
    setProjectFilter(value);
    setHistoryQuery((prev) => ({
      ...prev,
      page_no: 1,
      project_id: value ? parseInt(value, 10) : undefined,
    }));
  };

  // Rule editing
  const handleRuleChange = (
    ruleId: string,
    updates: Partial<{ enabled: boolean; threshold: Record<string, number>; cooldownSeconds: number }>
  ) => {
    setEditedRules((prev) =>
      prev.map((r) =>
        r.ruleId === ruleId
          ? {
              ...r,
              ...(updates.enabled !== undefined && { enabled: updates.enabled }),
              ...(updates.threshold && { threshold: updates.threshold }),
              ...(updates.cooldownSeconds !== undefined && { cooldownSeconds: updates.cooldownSeconds }),
            }
          : r
      )
    );
  };

  const handleSaveRules = async () => {
    setRulesSaving(true);
    try {
      const changedRules = editedRules
        .filter((edited) => {
          const original = rules.find((r) => r.ruleId === edited.ruleId);
          if (!original) return false;
          return (
            edited.enabled !== original.enabled ||
            JSON.stringify(edited.threshold) !== JSON.stringify(original.threshold) ||
            edited.cooldownSeconds !== original.cooldownSeconds
          );
        })
        .map((r) => ({
          ruleId: r.ruleId,
          enabled: r.enabled,
          threshold: r.threshold,
          cooldownSeconds: r.cooldownSeconds,
        }));

      if (changedRules.length === 0) {
        setSnack({ open: true, message: '没有需要保存的变更', severity: 'success' });
        setRulesSaving(false);
        return;
      }

      await updateRules({ rules: changedRules });
      setSnack({ open: true, message: '规则配置已保存', severity: 'success' });
    } catch (err) {
      setSnack({
        open: true,
        message: err instanceof Error ? err.message : '保存失败',
        severity: 'error',
      });
    } finally {
      setRulesSaving(false);
    }
  };

  // Channel editing
  const handleChannelChange = (channelId: string, updates: Partial<NotificationChannel>) => {
    setEditedChannels((prev) =>
      prev.map((c) => (c.channelId === channelId ? { ...c, ...updates } : c))
    );
  };

  const handleSaveChannels = async () => {
    setChannelsSaving(true);
    try {
      await updateChannels({
        channels: editedChannels.map((c) => ({
          channelId: c.channelId,
          name: c.name,
          channelType: c.channelType,
          enabled: c.enabled,
          config: c.config,
          severityFilter: c.severityFilter,
        })),
      });
      setSnack({ open: true, message: '渠道配置已保存', severity: 'success' });
    } catch (err) {
      setSnack({
        open: true,
        message: err instanceof Error ? err.message : '保存失败',
        severity: 'error',
      });
    } finally {
      setChannelsSaving(false);
    }
  };

  const handleTestNotification = async (channelId: string) => {
    try {
      const result = await testNotification({ channelId });
      return { success: result.success, responseTimeMs: result.responseTimeMs };
    } catch (err) {
      return { success: false, error: err instanceof Error ? err.message : '测试失败' };
    }
  };

  return (
    <Box sx={{ p: 3, maxWidth: 1000 }}>
      <Typography
        variant="h5"
        sx={{ fontWeight: 600, mb: 3, fontSize: '24px', lineHeight: '30px', letterSpacing: '-0.02em' }}
      >
        告警管理
      </Typography>

      <Tabs
        value={tab}
        onChange={(_, v) => setTab(v)}
        sx={{
          borderBottom: '1px solid #e1e2ed',
          '& .MuiTab-root': {
            textTransform: 'none',
            fontWeight: 500,
            fontSize: '14px',
            minHeight: 44,
          },
          '& .Mui-selected': {
            color: '#003ea8',
          },
          '& .MuiTabs-indicator': {
            backgroundColor: '#003ea8',
          },
        }}
      >
        <Tab label="告警历史" />
        <Tab label="告警规则" />
        <Tab label="通知渠道" />
      </Tabs>

      {/* Tab 0: Alert History */}
      <TabPanel value={tab} index={0}>
        {historyError && (
          <Alert severity="error" sx={{ mb: 2 }}>
            {historyError}
          </Alert>
        )}

        {/* Filters */}
        <Box sx={{ display: 'flex', gap: 2, mb: 3, flexWrap: 'wrap' }}>
          <TextField
            select
            label="严重级别"
            size="small"
            value={severityFilter}
            onChange={(e) => handleSeverityFilterChange(e.target.value)}
            sx={{
              width: 140,
              '& .MuiInputBase-root': { bgcolor: '#f3f3fe', borderRadius: '4px' },
            }}
          >
            <MenuItem value="">全部</MenuItem>
            <MenuItem value="critical">严重</MenuItem>
            <MenuItem value="warning">警告</MenuItem>
            <MenuItem value="info">信息</MenuItem>
          </TextField>

          <TextField
            label="项目 ID"
            size="small"
            type="number"
            value={projectFilter}
            onChange={(e) => handleProjectFilterChange(e.target.value)}
            placeholder="按项目筛选"
            sx={{
              width: 140,
              '& .MuiInputBase-root': { bgcolor: '#f3f3fe', borderRadius: '4px' },
            }}
            inputProps={{ min: 1 }}
          />
        </Box>

        <AlertHistoryTable
          data={history}
          loading={historyLoading}
          onPageChange={handlePageChange}
        />
      </TabPanel>

      {/* Tab 1: Alert Rules */}
      <TabPanel value={tab} index={1}>
        {rulesError && (
          <Alert severity="error" sx={{ mb: 2 }}>
            {rulesError}
          </Alert>
        )}

        {rulesLoading && editedRules.length === 0 ? (
          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {[...Array(3)].map((_, i) => (
              <Skeleton key={i} variant="rounded" height={120} />
            ))}
          </Box>
        ) : (
          <>
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              {editedRules.map((rule) => (
                <AlertRuleCard key={rule.ruleId} rule={rule} onChange={handleRuleChange} />
              ))}
            </Box>

            {editedRules.length > 0 && (
              <Box sx={{ mt: 3, display: 'flex', justifyContent: 'flex-end' }}>
                <Button
                  variant="contained"
                  startIcon={rulesSaving ? <CircularProgress size={16} color="inherit" /> : <SaveIcon />}
                  onClick={handleSaveRules}
                  disabled={rulesSaving}
                  sx={{
                    textTransform: 'none',
                    borderRadius: '4px',
                    background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
                    boxShadow: 'none',
                    '&:hover': {
                      background: 'linear-gradient(135deg, #0048c1 0%, #003ea8 100%)',
                      boxShadow: 'none',
                    },
                  }}
                >
                  保存规则
                </Button>
              </Box>
            )}
          </>
        )}
      </TabPanel>

      {/* Tab 2: Notification Channels */}
      <TabPanel value={tab} index={2}>
        {channelsError && (
          <Alert severity="error" sx={{ mb: 2 }}>
            {channelsError}
          </Alert>
        )}

        {channelsLoading && editedChannels.length === 0 ? (
          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {[...Array(2)].map((_, i) => (
              <Skeleton key={i} variant="rounded" height={200} />
            ))}
          </Box>
        ) : (
          <>
            {editedChannels.length === 0 ? (
              <Box sx={{ py: 6, textAlign: 'center' }}>
                <Typography color="text.secondary">暂无配置的通知渠道</Typography>
              </Box>
            ) : (
              <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                {editedChannels.map((channel) => (
                  <ChannelConfigCard
                    key={channel.channelId}
                    channel={channel}
                    onChange={handleChannelChange}
                    onTest={handleTestNotification}
                  />
                ))}
              </Box>
            )}

            {/* Available types info */}
            {availableTypes.filter((t) => t.status === 'coming_soon').length > 0 && (
              <Box sx={{ mt: 2 }}>
                <Typography sx={{ fontSize: '12px', color: '#737686' }}>
                  即将支持:{' '}
                  {availableTypes
                    .filter((t) => t.status === 'coming_soon')
                    .map((t) => t.name)
                    .join('、')}
                </Typography>
              </Box>
            )}

            {editedChannels.length > 0 && (
              <Box sx={{ mt: 3, display: 'flex', justifyContent: 'flex-end' }}>
                <Button
                  variant="contained"
                  startIcon={channelsSaving ? <CircularProgress size={16} color="inherit" /> : <SaveIcon />}
                  onClick={handleSaveChannels}
                  disabled={channelsSaving}
                  sx={{
                    textTransform: 'none',
                    borderRadius: '4px',
                    background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
                    boxShadow: 'none',
                    '&:hover': {
                      background: 'linear-gradient(135deg, #0048c1 0%, #003ea8 100%)',
                      boxShadow: 'none',
                    },
                  }}
                >
                  保存渠道配置
                </Button>
              </Box>
            )}
          </>
        )}
      </TabPanel>

      <Snackbar
        open={snack.open}
        autoHideDuration={4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert
          severity={snack.severity}
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
          sx={{ width: '100%' }}
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}
