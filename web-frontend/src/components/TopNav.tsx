import {
  AppBar,
  Toolbar,
  IconButton,
  Typography,
  Avatar,
  Box,
  Tooltip,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogContentText,
  DialogActions,
  Button,
  useMediaQuery,
  useTheme,
} from '@mui/material';
import MenuIcon from '@mui/icons-material/Menu';
import LogoutIcon from '@mui/icons-material/Logout';
import MusicNoteIcon from '@mui/icons-material/MusicNote';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuthStore } from '../store/auth';

interface TopNavProps {
  onMenuClick: () => void;
}

export default function TopNav({ onMenuClick }: TopNavProps) {
  const theme = useTheme();
  const isMobile = useMediaQuery(theme.breakpoints.down('md'));
  const { user, logout } = useAuthStore();
  const navigate = useNavigate();
  const [logoutDialogOpen, setLogoutDialogOpen] = useState(false);

  const handleLogout = () => {
    logout();
    navigate('/login', { replace: true });
    setLogoutDialogOpen(false);
  };

  const displayName = user?.displayName || user?.username || '';
  const avatarLetter = displayName.charAt(0).toUpperCase();

  return (
    <>
      <AppBar
        position="fixed"
        elevation={0}
        sx={{
          bgcolor: '#ffffff',
          color: '#191b23',
          zIndex: (t) => t.zIndex.drawer + 1,
          height: 48,
          boxShadow: 'none',
        }}
        role="banner"
      >
        <Toolbar
          sx={{
            px: { xs: 1, sm: 2, md: 3 },
            height: 48,
            minHeight: '48px !important',
          }}
        >
          {isMobile && (
            <IconButton
              edge="start"
              onClick={onMenuClick}
              aria-label="打开导航菜单"
              sx={{ mr: 1 }}
            >
              <MenuIcon />
            </IconButton>
          )}

          <MusicNoteIcon sx={{ color: '#0053db', fontSize: 24, mr: 1 }} />
          <Typography
            sx={{
              fontSize: '16px',
              fontWeight: 500,
              lineHeight: '22px',
              color: '#191b23',
            }}
          >
            Symphony Web
          </Typography>

          <Box sx={{ flexGrow: 1 }} />

          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
            <Avatar
              sx={{
                width: 32,
                height: 32,
                bgcolor: '#0053db',
                fontSize: 13,
                fontWeight: 500,
              }}
            >
              {avatarLetter}
            </Avatar>
            {!isMobile && (
              <Typography
                sx={{
                  fontSize: '14px',
                  fontWeight: 400,
                  lineHeight: '18px',
                  color: '#434655',
                }}
              >
                {displayName}
              </Typography>
            )}
            <Tooltip title="退出登录">
              <IconButton
                onClick={() => setLogoutDialogOpen(true)}
                aria-label="退出登录"
                size="small"
                sx={{
                  color: '#737686',
                  '&:hover': { color: '#191b23' },
                }}
              >
                <LogoutIcon fontSize="small" />
              </IconButton>
            </Tooltip>
          </Box>
        </Toolbar>
      </AppBar>

      <Dialog open={logoutDialogOpen} onClose={() => setLogoutDialogOpen(false)}>
        <DialogTitle>退出登录</DialogTitle>
        <DialogContent>
          <DialogContentText>确定要退出登录吗？</DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setLogoutDialogOpen(false)}>取消</Button>
          <Button onClick={handleLogout} variant="contained" color="primary">
            确定
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
