import { Box, Typography } from '@mui/material';
import type { AIGenerateStatus } from '../../store/issueStore';

interface StreamingDisplayProps {
  content: string;
  status: AIGenerateStatus;
  error: string | null;
}

export default function StreamingDisplay({
  content,
  status,
  error,
}: StreamingDisplayProps) {
  if (status === 'idle' && !content) {
    return null;
  }

  return (
    <Box
      sx={{
        backgroundColor: '#ededf9',
        borderRadius: '8px',
        p: 2,
        minHeight: 120,
        maxHeight: 400,
        overflow: 'auto',
        position: 'relative',
      }}
    >
      {status === 'generating' && (
        <Box
          sx={{
            position: 'absolute',
            top: 8,
            right: 8,
            display: 'flex',
            alignItems: 'center',
            gap: 0.5,
          }}
        >
          <Box
            sx={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              backgroundColor: '#0053db',
              animation: 'pulse 1.5s infinite',
              '@keyframes pulse': {
                '0%, 100%': { opacity: 1 },
                '50%': { opacity: 0.3 },
              },
            }}
          />
          <Typography variant="caption" color="text.secondary">
            生成中...
          </Typography>
        </Box>
      )}

      {error && (
        <Typography
          variant="body2"
          sx={{ color: '#ba1a1a', mb: 1 }}
        >
          {error}
        </Typography>
      )}

      <Box
        component="pre"
        sx={{
          fontFamily: '"JetBrains Mono", "Fira Code", monospace',
          fontSize: '13px',
          lineHeight: '20px',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
          margin: 0,
          color: '#191b23',
        }}
      >
        {content}
        {status === 'generating' && (
          <Box
            component="span"
            sx={{
              display: 'inline-block',
              width: '2px',
              height: '14px',
              backgroundColor: '#0053db',
              marginLeft: '1px',
              verticalAlign: 'text-bottom',
              animation: 'blink 1s step-end infinite',
              '@keyframes blink': {
                '0%, 100%': { opacity: 1 },
                '50%': { opacity: 0 },
              },
            }}
          />
        )}
      </Box>
    </Box>
  );
}
