import { Chip } from '@mui/material';
import type { ServiceStatus } from '../../types';

const statusConfig: Record<ServiceStatus, { label: string; color: string; bgColor: string }> = {
  running: { label: '运行中', color: '#1b6e2d', bgColor: '#dcfce7' },
  stopped: { label: '已停止', color: '#434655', bgColor: '#f3f3fe' },
  starting: { label: '启动中', color: '#7c5800', bgColor: '#fff3cd' },
  stopping: { label: '停止中', color: '#7c5800', bgColor: '#fff3cd' },
  error: { label: '异常', color: '#93000a', bgColor: '#ffdad6' },
  failed: { label: '失败', color: '#93000a', bgColor: '#ffdad6' },
};

interface ServiceStatusBadgeProps {
  status: ServiceStatus;
}

export default function ServiceStatusBadge({ status }: ServiceStatusBadgeProps) {
  const config = statusConfig[status];

  return (
    <Chip
      label={config.label}
      size="small"
      sx={{
        color: config.color,
        bgcolor: config.bgColor,
        fontWeight: 500,
        fontSize: '12px',
        height: 24,
        borderRadius: '4px',
      }}
      aria-label={`服务状态：${config.label}`}
    />
  );
}
