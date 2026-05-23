import { useState } from 'react';
import {
  Box,
  Typography,
  Switch,
  TextField,
  Chip,
  Checkbox,
  FormControlLabel,
  FormGroup,
} from '@mui/material';
import TestNotificationButton from './TestNotificationButton';
import type { NotificationChannel, Severity } from '../../types/alert';

const severityOptions: { value: Severity; label: string }[] = [
  { value: 'critical', label: '严重' },
  { value: 'warning', label: '警告' },
  { value: 'info', label: '信息' },
];

const channelTypeLabels: Record<string, string> = {
  dingtalk: '钉钉',
  slack: 'Slack',
  email: '邮件',
  webhook: 'Webhook',
};

interface ChannelConfigCardProps {
  channel: NotificationChannel;
  onChange: (channelId: string, updates: Partial<NotificationChannel>) => void;
  onTest: (channelId: string) => Promise<{ success: boolean; responseTimeMs?: number; error?: string }>;
}

export default function ChannelConfigCard({ channel, onChange, onTest }: ChannelConfigCardProps) {
  const [configValues, setConfigValues] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {};
    for (const [key, value] of Object.entries(channel.config)) {
      initial[key] = String(value ?? '');
    }
    return initial;
  });

  const handleConfigChange = (key: string, value: string) => {
    const updated = { ...configValues, [key]: value };
    setConfigValues(updated);
    onChange(channel.channelId, {
      config: updated as unknown as Record<string, unknown>,
    });
  };

  const handleSeverityToggle = (severity: Severity) => {
    const current = channel.severityFilter;
    const updated = current.includes(severity)
      ? current.filter((s) => s !== severity)
      : [...current, severity];
    onChange(channel.channelId, { severityFilter: updated });
  };

  return (
    <Box
      sx={{
        p: 3,
        bgcolor: '#ffffff',
        borderRadius: '8px',
        border: '1px solid #e1e2ed',
      }}
    >
      {/* Header */}
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 2 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
          <Typography sx={{ fontWeight: 500, fontSize: '14px', color: '#191b23' }}>
            {channel.name}
          </Typography>
          <Chip
            label={channelTypeLabels[channel.channelType] ?? channel.channelType}
            size="small"
            variant="outlined"
            sx={{ borderRadius: '4px', fontSize: '12px' }}
          />
          {channel.lastTestSuccess !== null && (
            <Chip
              label={channel.lastTestSuccess ? '连通' : '异常'}
              size="small"
              color={channel.lastTestSuccess ? 'success' : 'error'}
              variant="outlined"
              sx={{ borderRadius: '4px', fontSize: '11px' }}
            />
          )}
        </Box>
        <Switch
          checked={channel.enabled}
          onChange={(e) => onChange(channel.channelId, { enabled: e.target.checked })}
          size="small"
          inputProps={{ 'aria-label': `启用 ${channel.name}` }}
        />
      </Box>

      {/* Config fields */}
      <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, mb: 2 }}>
        {channel.channelType === 'dingtalk' && (
          <>
            <TextField
              label="Webhook URL"
              size="small"
              fullWidth
              value={configValues.webhook_url ?? ''}
              onChange={(e) => handleConfigChange('webhook_url', e.target.value)}
              disabled={!channel.enabled}
              placeholder="https://oapi.dingtalk.com/robot/send?access_token=..."
              sx={{
                '& .MuiInputBase-root': {
                  bgcolor: '#f3f3fe',
                  borderRadius: '4px',
                },
              }}
            />
            <TextField
              label="加签密钥（Secret）"
              size="small"
              fullWidth
              value={configValues.secret ?? ''}
              onChange={(e) => handleConfigChange('secret', e.target.value)}
              disabled={!channel.enabled}
              placeholder="SEC..."
              type="password"
              sx={{
                '& .MuiInputBase-root': {
                  bgcolor: '#f3f3fe',
                  borderRadius: '4px',
                },
              }}
            />
          </>
        )}
      </Box>

      {/* Severity filter */}
      <Box sx={{ mb: 2 }}>
        <Typography sx={{ fontSize: '12px', fontWeight: 500, color: '#737686', mb: 1 }}>
          接收告警级别
        </Typography>
        <FormGroup row>
          {severityOptions.map((opt) => (
            <FormControlLabel
              key={opt.value}
              control={
                <Checkbox
                  checked={channel.severityFilter.includes(opt.value)}
                  onChange={() => handleSeverityToggle(opt.value)}
                  size="small"
                  disabled={!channel.enabled}
                />
              }
              label={
                <Typography sx={{ fontSize: '13px' }}>{opt.label}</Typography>
              }
            />
          ))}
        </FormGroup>
      </Box>

      {/* Test button */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
        <TestNotificationButton
          channelId={channel.channelId}
          disabled={!channel.enabled}
          onTest={onTest}
        />
        {channel.lastTestAt && (
          <Typography sx={{ fontSize: '11px', color: '#737686' }}>
            上次测试: {new Date(channel.lastTestAt).toLocaleString('zh-CN')}
          </Typography>
        )}
      </Box>
    </Box>
  );
}
