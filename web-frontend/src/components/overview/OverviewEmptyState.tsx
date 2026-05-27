import { Box, Typography, Button } from '@mui/material';
import { useNavigate } from 'react-router-dom';
import RocketLaunchOutlinedIcon from '@mui/icons-material/RocketLaunchOutlined';

export default function OverviewEmptyState() {
  const navigate = useNavigate();

  return (
    <Box
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        py: 10,
        gap: 2,
      }}
    >
      <RocketLaunchOutlinedIcon
        sx={{ fontSize: 56, color: '#c3c6d7' }}
      />
      <Typography
        variant="body1"
        sx={{ color: '#737686', fontSize: '14px', textAlign: 'center' }}
      >
        暂无运行中的项目
      </Typography>
      <Button
        variant="contained"
        size="small"
        onClick={() => navigate('/projects')}
        sx={{
          textTransform: 'none',
          borderRadius: '4px',
          fontSize: '13px',
          fontWeight: 500,
          background: 'linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%)',
          '&:hover': {
            background: 'linear-gradient(135deg, #4f46e5 0%, #7c3aed 100%)',
          },
        }}
      >
        前往项目列表
      </Button>
    </Box>
  );
}
