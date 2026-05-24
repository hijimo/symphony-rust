import { useState } from 'react';
import {
  Box,
  Typography,
  Card,
  CardContent,
  TextField,
  Button,
  CircularProgress,
  Snackbar,
  Alert,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import { useNavigate } from 'react-router-dom';
import GitUrlInput, { parseGitUrl } from '../../components/projects/GitUrlInput';
import { useProjectStore } from '../../store/projectStore';

interface FormErrors {
  git_url?: string;
}

export default function CreateProjectPage() {
  const navigate = useNavigate();
  const { createProject } = useProjectStore();

  const [gitUrl, setGitUrl] = useState('');
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [defaultBranch, setDefaultBranch] = useState('');
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});
  const [snackError, setSnackError] = useState('');

  const validate = (): boolean => {
    const newErrors: FormErrors = {};
    if (!gitUrl.trim()) {
      newErrors.git_url = '请输入 Git URL';
    } else {
      const parsed = parseGitUrl(gitUrl.trim());
      if (!parsed.isValid) {
        newErrors.git_url = '无效的 Git URL 格式，请输入 HTTPS 或 SSH 格式的地址';
      }
    }
    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async () => {
    if (!validate()) return;

    setLoading(true);
    try {
      const project = await createProject({
        git_url: gitUrl.trim(),
        name: name.trim() || undefined,
        description: description.trim() || undefined,
        default_branch: defaultBranch.trim() || undefined,
      });
      navigate(`/projects/${project.id}/kanban`, { replace: true });
    } catch (err: any) {
      setSnackError(err?.message || '创建项目失败');
    } finally {
      setLoading(false);
    }
  };

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, mb: 3 }}>
        <Button
          variant="text"
          startIcon={<ArrowBackIcon />}
          onClick={() => navigate('/projects')}
          sx={{ color: '#434655', minWidth: 'auto', px: 1 }}
        >
          返回
        </Button>
        <Typography variant="h5" color="text.primary">
          创建项目
        </Typography>
      </Box>

      <Card sx={{ maxWidth: 640, border: '1px solid #c3c6d7' }}>
        <CardContent sx={{ p: 3 }}>
          <Typography variant="body1" color="text.secondary" sx={{ mb: 3 }}>
            通过 Git 仓库地址创建项目，系统将自动识别平台和仓库信息。
          </Typography>

          {/* Git URL Input */}
          <Box sx={{ mb: 3 }}>
            <GitUrlInput
              value={gitUrl}
              onChange={(v) => {
                setGitUrl(v);
                if (errors.git_url) setErrors({});
              }}
              error={errors.git_url}
              disabled={loading}
            />
          </Box>

          {/* Optional fields */}
          <Typography
            variant="subtitle2"
            color="text.secondary"
            sx={{ mb: 2, mt: 1 }}
          >
            可选配置
          </Typography>

          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2.5 }}>
            <TextField
              label="项目名称"
              placeholder="留空则使用仓库名"
              value={name}
              onChange={(e) => setName(e.target.value)}
              fullWidth
              disabled={loading}
              helperText="自定义项目显示名称"
            />
            <TextField
              label="项目描述"
              placeholder="简要描述项目用途"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              fullWidth
              disabled={loading}
              multiline
              minRows={2}
              maxRows={4}
            />
            <TextField
              label="默认分支"
              placeholder="main"
              value={defaultBranch}
              onChange={(e) => setDefaultBranch(e.target.value)}
              fullWidth
              disabled={loading}
              helperText="留空默认为 main"
            />
          </Box>

          {/* Submit */}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 4, gap: 1.5 }}>
            <Button
              variant="outlined"
              onClick={() => navigate('/projects')}
              disabled={loading}
              sx={{ borderColor: '#c3c6d7', color: '#434655' }}
            >
              取消
            </Button>
            <Button
              variant="contained"
              onClick={handleSubmit}
              disabled={loading || !gitUrl.trim()}
              startIcon={loading ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              创建项目
            </Button>
          </Box>
        </CardContent>
      </Card>

      {/* Error Snackbar */}
      <Snackbar
        open={!!snackError}
        autoHideDuration={6000}
        onClose={() => setSnackError('')}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert
          severity="error"
          onClose={() => setSnackError('')}
          variant="filled"
          sx={{ borderRadius: '4px' }}
        >
          {snackError}
        </Alert>
      </Snackbar>
    </Box>
  );
}
