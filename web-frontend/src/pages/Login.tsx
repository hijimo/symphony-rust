import { useState, FormEvent } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import {
  Box,
  Card,
  CardContent,
  TextField,
  Button,
  Typography,
  InputAdornment,
  IconButton,
  Alert,
  Collapse,
  CircularProgress,
} from '@mui/material';
import MusicNoteIcon from '@mui/icons-material/MusicNote';
import VisibilityOutlinedIcon from '@mui/icons-material/VisibilityOutlined';
import VisibilityOffOutlinedIcon from '@mui/icons-material/VisibilityOffOutlined';
import { login } from '../api/auth';
import { useAuthStore } from '../store/auth';

export default function Login() {
  const navigate = useNavigate();
  const location = useLocation();
  const authLogin = useAuthStore((s) => s.login);

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [fieldErrors, setFieldErrors] = useState<{ username?: string; password?: string }>({});

  const validate = (): boolean => {
    const errors: { username?: string; password?: string } = {};
    if (!username.trim()) errors.username = '请输入用户名';
    if (!password) errors.password = '请输入密码';
    setFieldErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (!validate()) return;

    setLoading(true);
    try {
      const res = await login({ username: username.trim(), password });
      authLogin(res.token, res.user, res.expiresAt);
      const from = (location.state as { from?: { pathname: string } })?.from?.pathname;
      const target = from || (res.user.role === 'admin' ? '/admin/users' : '/settings');
      navigate(target, { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : '登录失败，请重试');
    } finally {
      setLoading(false);
    }
  };

  return (
    <Box
      sx={{
        minHeight: '100vh',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        bgcolor: '#faf8ff',
        px: 2,
      }}
    >
      <Card
        elevation={0}
        sx={{
          width: '100%',
          maxWidth: 400,
          p: { xs: 3, sm: 5 },
          borderRadius: '8px',
          bgcolor: '#ffffff',
          boxShadow: 'none',
        }}
      >
        <CardContent sx={{ p: 0, '&:last-child': { pb: 0 } }}>
          <Box sx={{ textAlign: 'center', mb: 4 }}>
            <MusicNoteIcon
              sx={{ fontSize: { xs: 40, sm: 48 }, color: '#0053db', mb: 1.5 }}
            />
            <Typography
              variant="h4"
              sx={{
                color: '#191b23',
                fontSize: '24px',
                fontWeight: 600,
                lineHeight: '30px',
                letterSpacing: '-0.02em',
              }}
            >
              Symphony Web
            </Typography>
            <Typography
              variant="subtitle1"
              sx={{
                color: '#434655',
                mt: 0.5,
                fontSize: '16px',
                fontWeight: 500,
                lineHeight: '22px',
              }}
            >
              统一工作流管理平台
            </Typography>
          </Box>

          <Box component="form" onSubmit={handleSubmit} noValidate>
            <TextField
              fullWidth
              variant="filled"
              label="用户名"
              placeholder="请输入用户名"
              value={username}
              onChange={(e) => {
                setUsername(e.target.value);
                if (fieldErrors.username) setFieldErrors((p) => ({ ...p, username: undefined }));
              }}
              error={!!fieldErrors.username}
              helperText={fieldErrors.username}
              disabled={loading}
              autoFocus
              autoComplete="username"
              sx={{
                mb: 2.5,
                '& .MuiFilledInput-root': {
                  borderRadius: '4px',
                  bgcolor: '#f3f3fe',
                  '&:hover': { bgcolor: '#f3f3fe' },
                  '&.Mui-focused': { bgcolor: '#f3f3fe' },
                  '&::before': { borderBottom: '2px solid transparent' },
                  '&::after': { borderBottom: '2px solid #0053db' },
                },
              }}
            />

            <TextField
              fullWidth
              variant="filled"
              label="密码"
              placeholder="请输入密码"
              type={showPassword ? 'text' : 'password'}
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
                if (fieldErrors.password) setFieldErrors((p) => ({ ...p, password: undefined }));
              }}
              error={!!fieldErrors.password}
              helperText={fieldErrors.password}
              disabled={loading}
              autoComplete="current-password"
              slotProps={{
                input: {
                  endAdornment: (
                    <InputAdornment position="end">
                      <IconButton
                        onClick={() => setShowPassword(!showPassword)}
                        edge="end"
                        aria-label={showPassword ? '隐藏密码' : '显示密码'}
                        size="small"
                      >
                        {showPassword ? (
                          <VisibilityOffOutlinedIcon />
                        ) : (
                          <VisibilityOutlinedIcon />
                        )}
                      </IconButton>
                    </InputAdornment>
                  ),
                },
              }}
              sx={{
                mb: 3,
                '& .MuiFilledInput-root': {
                  borderRadius: '4px',
                  bgcolor: '#f3f3fe',
                  '&:hover': { bgcolor: '#f3f3fe' },
                  '&.Mui-focused': { bgcolor: '#f3f3fe' },
                  '&::before': { borderBottom: '2px solid transparent' },
                  '&::after': { borderBottom: '2px solid #0053db' },
                },
              }}
            />

            <Button
              type="submit"
              variant="contained"
              size="large"
              fullWidth
              disabled={loading}
              aria-busy={loading}
              sx={{
                height: 44,
                borderRadius: '4px',
                fontSize: '14px',
                fontWeight: 500,
                lineHeight: '18px',
                background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
                color: '#ffffff',
                border: 'none',
                boxShadow: 'none',
                textTransform: 'none',
                '&:hover': {
                  background: 'linear-gradient(135deg, #0048c1 0%, #003da6 100%)',
                  boxShadow: 'none',
                },
                '&.Mui-disabled': {
                  background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
                  opacity: 0.6,
                  color: '#ffffff',
                },
              }}
            >
              {loading ? <CircularProgress size={24} color="inherit" /> : '登录'}
            </Button>

            <Collapse in={!!error}>
              <Alert severity="error" sx={{ mt: 2 }} role="alert" aria-live="polite">
                {error}
              </Alert>
            </Collapse>
          </Box>
        </CardContent>
      </Card>

      <Typography
        variant="caption"
        sx={{
          mt: 3,
          color: '#737686',
          fontSize: '11px',
          fontWeight: 500,
          lineHeight: '16px',
        }}
      >
        v0.1.0 - Phase 1
      </Typography>
    </Box>
  );
}
