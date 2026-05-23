import { Button } from '@mui/material';
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome';
import StopIcon from '@mui/icons-material/Stop';
import type { AIGenerateStatus } from '../../store/issueStore';

interface AiGenerateButtonProps {
  status: AIGenerateStatus;
  disabled?: boolean;
  onGenerate: () => void;
  onStop: () => void;
}

export default function AiGenerateButton({
  status,
  disabled,
  onGenerate,
  onStop,
}: AiGenerateButtonProps) {
  if (status === 'generating') {
    return (
      <Button
        variant="contained"
        onClick={onStop}
        startIcon={<StopIcon />}
        sx={{
          background: '#832600',
          color: '#ffffff',
          '&:hover': {
            background: '#ac3500',
          },
        }}
      >
        停止生成
      </Button>
    );
  }

  return (
    <Button
      variant="contained"
      onClick={onGenerate}
      disabled={disabled}
      startIcon={<AutoAwesomeIcon />}
      sx={{
        background: 'linear-gradient(135deg, #0053db 0%, #003ea8 100%)',
        color: '#ffffff',
        '&:hover': {
          background: 'linear-gradient(135deg, #003ea8 0%, #00174b 100%)',
        },
      }}
    >
      AI 生成
    </Button>
  );
}
