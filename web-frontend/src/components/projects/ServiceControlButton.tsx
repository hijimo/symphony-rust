import { useState } from 'react';
import { IconButton, Tooltip, CircularProgress } from '@mui/material';
import PlayArrowIcon from '@mui/icons-material/PlayArrow';
import StopIcon from '@mui/icons-material/Stop';
import RestartAltIcon from '@mui/icons-material/RestartAlt';
import type { ServiceStatus } from '../../types';

interface ServiceControlButtonProps {
  status: ServiceStatus;
  onStart: () => Promise<void>;
  onStop: () => Promise<void>;
  onRestart?: () => Promise<void>;
  size?: 'small' | 'medium';
}

export default function ServiceControlButton({
  status,
  onStart,
  onStop,
  onRestart,
  size = 'small',
}: ServiceControlButtonProps) {
  const [loading, setLoading] = useState(false);

  const handleAction = async (action: () => Promise<void>) => {
    setLoading(true);
    try {
      await action();
    } finally {
      setLoading(false);
    }
  };

  const isTransitioning = status === 'starting' || status === 'stopping';

  if (loading || isTransitioning) {
    return (
      <IconButton size={size} disabled>
        <CircularProgress size={size === 'small' ? 16 : 20} />
      </IconButton>
    );
  }

  if (status === 'running') {
    return (
      <>
        <Tooltip title="停止服务">
          <IconButton
            size={size}
            onClick={() => handleAction(onStop)}
            aria-label="停止服务"
            sx={{ color: '#ba1a1a' }}
          >
            <StopIcon fontSize={size} />
          </IconButton>
        </Tooltip>
        {onRestart && (
          <Tooltip title="重启服务">
            <IconButton
              size={size}
              onClick={() => handleAction(onRestart)}
              aria-label="重启服务"
              sx={{ color: '#434655' }}
            >
              <RestartAltIcon fontSize={size} />
            </IconButton>
          </Tooltip>
        )}
      </>
    );
  }

  // stopped, error, failed
  return (
    <Tooltip title="启动服务">
      <IconButton
        size={size}
        onClick={() => handleAction(onStart)}
        aria-label="启动服务"
        sx={{ color: '#1b6e2d' }}
      >
        <PlayArrowIcon fontSize={size} />
      </IconButton>
    </Tooltip>
  );
}
