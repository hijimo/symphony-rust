import { useState } from 'react';
import { Button, Typography, Box, CircularProgress } from '@mui/material';
import SendIcon from '@mui/icons-material/Send';
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline';
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline';

interface TestNotificationButtonProps {
  channelId: string;
  disabled?: boolean;
  onTest: (channelId: string) => Promise<{ success: boolean; responseTimeMs?: number; error?: string }>;
}

export default function TestNotificationButton({
  channelId,
  disabled = false,
  onTest,
}: TestNotificationButtonProps) {
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{ success: boolean; responseTimeMs?: number; error?: string } | null>(null);

  const handleTest = async () => {
    setLoading(true);
    setResult(null);
    try {
      const res = await onTest(channelId);
      setResult(res);
    } catch (err) {
      setResult({ success: false, error: err instanceof Error ? err.message : '测试失败' });
    } finally {
      setLoading(false);
    }
  };

  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
      <Button
        variant="outlined"
        size="small"
        startIcon={loading ? <CircularProgress size={14} /> : <SendIcon sx={{ fontSize: 16 }} />}
        onClick={handleTest}
        disabled={disabled || loading}
        sx={{
          borderRadius: '4px',
          textTransform: 'none',
          fontSize: '13px',
          borderColor: '#c3c6d7',
          color: '#434655',
          '&:hover': {
            borderColor: '#003ea8',
            color: '#003ea8',
          },
        }}
      >
        测试通知
      </Button>
      {result && (
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
          {result.success ? (
            <>
              <CheckCircleOutlineIcon sx={{ fontSize: 16, color: '#2e7d32' }} />
              <Typography sx={{ fontSize: '12px', color: '#2e7d32' }}>
                成功{result.responseTimeMs ? ` (${result.responseTimeMs}ms)` : ''}
              </Typography>
            </>
          ) : (
            <>
              <ErrorOutlineIcon sx={{ fontSize: 16, color: '#ba1a1a' }} />
              <Typography sx={{ fontSize: '12px', color: '#ba1a1a' }}>
                {result.error ?? '发送失败'}
              </Typography>
            </>
          )}
        </Box>
      )}
    </Box>
  );
}
