import {
  Box,
  Typography,
  Switch,
  TextField,
} from '@mui/material';
import SeverityBadge from './SeverityBadge';
import type { AlertRule } from '../../types/alert';

interface AlertRuleCardProps {
  rule: AlertRule;
  onChange: (ruleId: string, updates: Partial<{ enabled: boolean; threshold: Record<string, number>; cooldownSeconds: number }>) => void;
}

/** Threshold field labels for known rules */
const thresholdLabels: Record<string, Record<string, string>> = {
  task_timeout: { timeout_minutes: '超时阈值（分钟）' },
  concurrency_saturation: { saturation_minutes: '饱和持续时间（分钟）' },
  consecutive_failures: { failure_count: '连续失败次数' },
  api_unreachable: { failure_count: '连续失败次数' },
};

export default function AlertRuleCard({ rule, onChange }: AlertRuleCardProps) {
  const labels = thresholdLabels[rule.ruleId] ?? {};
  const thresholdKeys = Object.keys(rule.threshold);

  return (
    <Box
      sx={{
        p: 3,
        bgcolor: '#ffffff',
        borderRadius: '8px',
        border: '1px solid #e1e2ed',
      }}
    >
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
          <Typography sx={{ fontWeight: 500, fontSize: '14px', color: '#191b23' }}>
            {rule.name}
          </Typography>
          <SeverityBadge severity={rule.severity} />
        </Box>
        <Switch
          checked={rule.enabled}
          onChange={(e) => onChange(rule.ruleId, { enabled: e.target.checked })}
          size="small"
          inputProps={{ 'aria-label': `启用 ${rule.name}` }}
        />
      </Box>

      <Typography sx={{ fontSize: '12px', color: '#737686', mb: 2 }}>
        {rule.description}
      </Typography>

      <Box sx={{ display: 'flex', gap: 2, flexWrap: 'wrap' }}>
        {thresholdKeys.map((key) => (
          <TextField
            key={key}
            label={labels[key] ?? key}
            type="number"
            size="small"
            value={rule.threshold[key] ?? ''}
            onChange={(e) => {
              const val = parseInt(e.target.value, 10);
              if (!isNaN(val) && val > 0) {
                onChange(rule.ruleId, {
                  threshold: { ...rule.threshold, [key]: val },
                });
              }
            }}
            disabled={!rule.enabled}
            sx={{
              width: 180,
              '& .MuiInputBase-root': {
                bgcolor: '#f3f3fe',
                borderRadius: '4px',
              },
            }}
            inputProps={{ min: 1 }}
          />
        ))}
        <TextField
          label="冷却时间（秒）"
          type="number"
          size="small"
          value={rule.cooldownSeconds}
          onChange={(e) => {
            const val = parseInt(e.target.value, 10);
            if (!isNaN(val) && val >= 60 && val <= 3600) {
              onChange(rule.ruleId, { cooldownSeconds: val });
            }
          }}
          disabled={!rule.enabled}
          sx={{
            width: 180,
            '& .MuiInputBase-root': {
              bgcolor: '#f3f3fe',
              borderRadius: '4px',
            },
          }}
          inputProps={{ min: 60, max: 3600 }}
          helperText="60-3600"
        />
      </Box>
    </Box>
  );
}
