import { Chip } from '@mui/material';
import type { Severity } from '../../types/alert';

const severityConfig: Record<Severity, { label: string; color: string; bg: string }> = {
  critical: { label: '严重', color: '#93000a', bg: '#ffdad6' },
  warning: { label: '警告', color: '#7a5900', bg: '#ffefd6' },
  info: { label: '信息', color: '#003ea8', bg: '#dbe1ff' },
};

interface SeverityBadgeProps {
  severity: Severity;
  size?: 'small' | 'medium';
}

export default function SeverityBadge({ severity, size = 'small' }: SeverityBadgeProps) {
  const config = severityConfig[severity] ?? severityConfig.info;

  return (
    <Chip
      label={config.label}
      size={size}
      sx={{
        fontWeight: 500,
        fontSize: '12px',
        lineHeight: '16px',
        letterSpacing: '0.02em',
        color: config.color,
        bgcolor: config.bg,
        borderRadius: '4px',
        height: size === 'small' ? 22 : 28,
      }}
    />
  );
}
