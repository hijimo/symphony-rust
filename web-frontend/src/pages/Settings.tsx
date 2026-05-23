import { useState, useEffect } from 'react';
import {
  Box,
  Typography,
  Card,
  CardContent,
  TextField,
  Button,
  Avatar,
  Divider,
  Chip,
  Snackbar,
  Alert,
  CircularProgress,
  Skeleton,
} from '@mui/material';
import {
  PersonOutline,
  VpnKeyOutlined,
  CheckCircle,
  RadioButtonUnchecked,
} from '@mui/icons-material';
import PasswordField from '../components/PasswordField';
import { getProfile, updateProfile, getConfig, updateConfig, changePassword } from '../api/user';
import type { UserConfig } from '../types';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

export default function Settings() {
  const [loading, setLoading] = useState(true);
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  // Profile state
  const [displayName, setDisplayName] = useState('');
  const [username, setUsername] = useState('');
  const [originalDisplayName, setOriginalDisplayName] = useState('');
  const [profileSaving, setProfileSaving] = useState(false);

  // Password state
  const [oldPassword, setOldPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [passwordErrors, setPasswordErrors] = useState<Record<string, string>>({});
  const [passwordSaving, setPasswordSaving] = useState(false);

  // Token state
  const [config, setConfig] = useState<UserConfig | null>(null);
  const [gitlabToken, setGitlabToken] = useState('');
  const [gitlabHost, setGitlabHost] = useState('');
  const [githubToken, setGithubToken] = useState('');
  const [tokenSaving, setTokenSaving] = useState(false);

  useEffect(() => {
    const load = async () => {
      try {
        const [profile, cfg] = await Promise.all([getProfile(), getConfig()]);
        setDisplayName(profile.displayName || '');
        setOriginalDisplayName(profile.displayName || '');
        setUsername(profile.username || '');
        setConfig(cfg);
        setGitlabHost(cfg.gitlabHost || '');
      } catch (err: any) {
        showSnack(err?.message || '加载数据失败', 'error');
      } finally {
        setLoading(false);
      }
    };
    load();
  }, []);

  const showSnack = (message: string, severity: 'success' | 'error') => {
    setSnack({ open: true, message, severity });
  };

  // Profile save
  const handleProfileSave = async () => {
    if (!displayName.trim()) return;
    setProfileSaving(true);
    try {
      await updateProfile({ displayName: displayName.trim() });
      setOriginalDisplayName(displayName.trim());
      showSnack('个人信息已更新', 'success');
    } catch (err: any) {
      showSnack(err?.message || '更新失败', 'error');
    } finally {
      setProfileSaving(false);
    }
  };

  // Password save
  const handlePasswordSave = async () => {
    const errors: Record<string, string> = {};
    if (!oldPassword) errors.oldPassword = '请输入当前密码';
    if (!newPassword || newPassword.length < 6) errors.newPassword = '新密码至少6个字符';
    if (newPassword !== confirmPassword) errors.confirmPassword = '两次输入的密码不一致';
    setPasswordErrors(errors);
    if (Object.keys(errors).length > 0) return;

    setPasswordSaving(true);
    try {
      await changePassword({ oldPassword, newPassword });
      showSnack('密码修改成功', 'success');
      setOldPassword('');
      setNewPassword('');
      setConfirmPassword('');
    } catch (err: any) {
      const msg = err?.message || '修改密码失败';
      if (msg.includes('密码') && msg.includes('不正确')) {
        setPasswordErrors({ oldPassword: '当前密码不正确' });
      } else {
        showSnack(msg, 'error');
      }
    } finally {
      setPasswordSaving(false);
    }
  };

  // Token save
  const handleTokenSave = async () => {
    setTokenSaving(true);
    try {
      const data: Record<string, string> = {};
      if (gitlabToken) data.gitlabToken = gitlabToken;
      if (gitlabHost !== (config?.gitlabHost || '')) data.gitlabHost = gitlabHost;
      if (githubToken) data.githubToken = githubToken;

      await updateConfig(data);
      const newConfig = await getConfig();
      setConfig(newConfig);
      setGitlabToken('');
      setGithubToken('');
      setGitlabHost(newConfig.gitlabHost || '');
      showSnack('Token 配置已保存', 'success');
    } catch (err: any) {
      showSnack(err?.message || '保存失败', 'error');
    } finally {
      setTokenSaving(false);
    }
  };

  const profileChanged = displayName.trim() !== originalDisplayName;
  const passwordFilled = oldPassword || newPassword || confirmPassword;
  const tokenChanged = gitlabToken || githubToken || gitlabHost !== (config?.gitlabHost || '');

  if (loading) {
    return (
      <Box>
        <Skeleton variant="text" width={200} height={40} sx={{ mb: 3 }} />
        <Skeleton variant="rounded" height={300} sx={{ mb: 3, borderRadius: '8px' }} />
        <Skeleton variant="rounded" height={400} sx={{ borderRadius: '8px' }} />
      </Box>
    );
  }

  return (
    <Box>
      {/* Page Title */}
      <Typography variant="h5" color="text.primary" sx={{ mb: 3 }}>
        个人设置
      </Typography>

      {/* Profile Card */}
      <Card sx={{ mb: 3 }}>
        <CardContent sx={{ p: 3 }}>
          {/* Card Header */}
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, mb: 3 }}>
            <Avatar sx={{ bgcolor: '#0053db', width: 40, height: 40 }}>
              <PersonOutline />
            </Avatar>
            <Typography variant="h6">
              个人信息
            </Typography>
          </Box>

          {/* Display Name Field */}
          <TextField
            label="显示名"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            fullWidth
            sx={{ maxWidth: 400, mb: 2.5 }}
            helperText="其他用户看到的名称"
          />

          {/* Username Field (read-only) */}
          {username && (
            <TextField
              label="用户名"
              value={username}
              fullWidth
              sx={{ maxWidth: 400, mb: 2.5 }}
              InputProps={{ readOnly: true }}
              helperText="用户名不可修改"
            />
          )}

          {/* Profile Save Button */}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
            <Button
              variant="contained"
              onClick={handleProfileSave}
              disabled={!profileChanged || profileSaving}
              startIcon={profileSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              保存
            </Button>
          </Box>

          <Divider sx={{ my: 3 }} />

          {/* Password Section */}
          <Typography variant="subtitle2" color="text.secondary" sx={{ mb: 0.5 }}>
            修改密码
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 2.5, display: 'block' }}>
            留空则不修改密码
          </Typography>

          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, maxWidth: 400 }}>
            <PasswordField
              label="当前密码"
              value={oldPassword}
              onChange={(e) => setOldPassword(e.target.value)}
              fullWidth
              error={!!passwordErrors.oldPassword}
              helperText={passwordErrors.oldPassword}
              placeholder="请输入当前密码"
            />
            <PasswordField
              label="新密码"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              fullWidth
              error={!!passwordErrors.newPassword}
              helperText={passwordErrors.newPassword}
              placeholder="请输入新密码（至少6位）"
            />
            <PasswordField
              label="确认新密码"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              fullWidth
              error={!!passwordErrors.confirmPassword}
              helperText={passwordErrors.confirmPassword}
              placeholder="请再次输入新密码"
            />
          </Box>

          {/* Password Save Button */}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
            <Button
              variant="contained"
              onClick={handlePasswordSave}
              disabled={!passwordFilled || passwordSaving}
              startIcon={passwordSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              修改密码
            </Button>
          </Box>
        </CardContent>
      </Card>

      {/* Token Config Card */}
      <Card>
        <CardContent sx={{ p: 3 }}>
          {/* Card Header */}
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, mb: 0.5 }}>
            <Avatar sx={{ bgcolor: '#0053db', width: 40, height: 40 }}>
              <VpnKeyOutlined />
            </Avatar>
            <Box>
              <Typography variant="h6">
                Token 配置
              </Typography>
              <Typography variant="body2" color="text.secondary">
                配置第三方平台访问令牌
              </Typography>
            </Box>
          </Box>

          <Box sx={{ mt: 3 }}>
            {/* GitLab Token */}
            <Box sx={{ mb: 2 }}>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5 }}>
                <Typography variant="subtitle2">GitLab Token</Typography>
                <Chip
                  size="small"
                  variant="outlined"
                  color={config?.hasGitlabToken ? 'success' : 'default'}
                  icon={config?.hasGitlabToken ? <CheckCircle /> : <RadioButtonUnchecked />}
                  label={config?.hasGitlabToken ? '已配置' : '未配置'}
                  aria-label={`GitLab Token 状态：${config?.hasGitlabToken ? '已配置' : '未配置'}`}
                />
              </Box>
              <PasswordField
                fullWidth
                value={gitlabToken}
                onChange={(e) => setGitlabToken(e.target.value)}
                placeholder={config?.hasGitlabToken ? '••••••••（已保存，输入新值覆盖）' : '请输入 GitLab Personal Access Token'}
                helperText="用于访问 GitLab API，需要 api 和 read_repository 权限"
              />
            </Box>

            {/* GitLab Host */}
            <Box sx={{ mb: 3 }}>
              <TextField
                label="GitLab Host"
                fullWidth
                value={gitlabHost}
                onChange={(e) => setGitlabHost(e.target.value)}
                placeholder="https://gitlab.com"
                helperText="自建 GitLab 实例地址，使用 gitlab.com 可留空"
              />
            </Box>

            <Divider sx={{ mb: 3 }} />

            {/* GitHub Token */}
            <Box sx={{ mb: 2 }}>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5 }}>
                <Typography variant="subtitle2">GitHub Token</Typography>
                <Chip
                  size="small"
                  variant="outlined"
                  color={config?.hasGithubToken ? 'success' : 'default'}
                  icon={config?.hasGithubToken ? <CheckCircle /> : <RadioButtonUnchecked />}
                  label={config?.hasGithubToken ? '已配置' : '未配置'}
                  aria-label={`GitHub Token 状态：${config?.hasGithubToken ? '已配置' : '未配置'}`}
                />
              </Box>
              <PasswordField
                fullWidth
                value={githubToken}
                onChange={(e) => setGithubToken(e.target.value)}
                placeholder={config?.hasGithubToken ? '••••••••（已保存，输入新值覆盖）' : '请输入 GitHub Personal Access Token'}
                helperText="用于访问 GitHub API，需要 repo 权限"
              />
            </Box>
          </Box>

          {/* Token Save Button */}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
            <Button
              variant="contained"
              onClick={handleTokenSave}
              disabled={!tokenChanged || tokenSaving}
              startIcon={tokenSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              保存 Token 配置
            </Button>
          </Box>
        </CardContent>
      </Card>

      {/* Snackbar */}
      <Snackbar
        open={snack.open}
        autoHideDuration={snack.severity === 'error' ? 6000 : 4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert
          severity={snack.severity}
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
          variant="filled"
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}
