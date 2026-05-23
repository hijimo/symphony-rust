import { useState, useEffect, useCallback } from 'react';
import {
  Box,
  Typography,
  Button,
  TextField,
  Select,
  MenuItem,
  FormControl,
  InputLabel,
  InputAdornment,
  Skeleton,
  Snackbar,
  Alert,
  IconButton,
  Tooltip,
} from '@mui/material';
import { Add, Search, Refresh, FolderOutlined } from '@mui/icons-material';
import { useNavigate } from 'react-router-dom';
import ProjectCard from '../../components/projects/ProjectCard';
import { useProjectStore } from '../../store/projectStore';
import type { ProjectPlatform, ServiceStatus } from '../../types';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

export default function ProjectListPage() {
  const navigate = useNavigate();
  const { projects, loading, pagination, fetchProjects, startService, stopService } =
    useProjectStore();

  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const [platformFilter, setPlatformFilter] = useState<ProjectPlatform | ''>('');
  const [statusFilter, setStatusFilter] = useState<ServiceStatus | ''>('');
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  const loadProjects = useCallback(() => {
    fetchProjects({
      pageNo: 1,
      pageSize: 20,
      search: search || undefined,
      platform: platformFilter || undefined,
      status: statusFilter || undefined,
    }).catch((err) => {
      setSnack({ open: true, message: err?.message || '获取项目列表失败', severity: 'error' });
    });
  }, [fetchProjects, search, platformFilter, statusFilter]);

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  // Debounced search
  useEffect(() => {
    const timer = setTimeout(() => {
      setSearch(searchInput);
    }, 300);
    return () => clearTimeout(timer);
  }, [searchInput]);

  const handleStart = async (id: number) => {
    try {
      await startService(id);
      setSnack({ open: true, message: '服务启动成功', severity: 'success' });
    } catch (err: any) {
      setSnack({ open: true, message: err?.message || '启动失败', severity: 'error' });
    }
  };

  const handleStop = async (id: number) => {
    try {
      await stopService(id);
      setSnack({ open: true, message: '服务已停止', severity: 'success' });
    } catch (err: any) {
      setSnack({ open: true, message: err?.message || '停止失败', severity: 'error' });
    }
  };

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 3 }}>
        <Typography variant="h5" color="text.primary">
          项目
        </Typography>
        <Button
          variant="contained"
          startIcon={<Add />}
          onClick={() => navigate('/projects/new')}
        >
          创建项目
        </Button>
      </Box>

      {/* Toolbar */}
      <Box
        sx={{
          display: 'flex',
          gap: 1.5,
          alignItems: 'center',
          mb: 3,
          flexWrap: 'wrap',
        }}
      >
        <TextField
          size="small"
          placeholder="搜索项目名称或 Git URL..."
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          sx={{ width: { xs: '100%', sm: 280 } }}
          aria-label="搜索项目"
          variant="filled"
          slotProps={{
            input: {
              startAdornment: (
                <InputAdornment position="start">
                  <Search color="action" />
                </InputAdornment>
              ),
            },
          }}
        />
        <FormControl size="small" sx={{ minWidth: 120 }} variant="filled">
          <InputLabel>平台</InputLabel>
          <Select
            value={platformFilter}
            label="平台"
            onChange={(e) => setPlatformFilter(e.target.value as ProjectPlatform | '')}
          >
            <MenuItem value="">全部平台</MenuItem>
            <MenuItem value="gitlab">GitLab</MenuItem>
            <MenuItem value="github">GitHub</MenuItem>
          </Select>
        </FormControl>
        <FormControl size="small" sx={{ minWidth: 120 }} variant="filled">
          <InputLabel>状态</InputLabel>
          <Select
            value={statusFilter}
            label="状态"
            onChange={(e) => setStatusFilter(e.target.value as ServiceStatus | '')}
          >
            <MenuItem value="">全部状态</MenuItem>
            <MenuItem value="running">运行中</MenuItem>
            <MenuItem value="stopped">已停止</MenuItem>
            <MenuItem value="starting">启动中</MenuItem>
            <MenuItem value="stopping">停止中</MenuItem>
            <MenuItem value="error">异常</MenuItem>
            <MenuItem value="failed">失败</MenuItem>
          </Select>
        </FormControl>
        <Tooltip title="刷新">
          <IconButton onClick={loadProjects} aria-label="刷新列表">
            <Refresh />
          </IconButton>
        </Tooltip>
      </Box>

      {/* Content */}
      {loading ? (
        <Box
          sx={{
            display: 'grid',
            gridTemplateColumns: {
              xs: '1fr',
              sm: 'repeat(2, 1fr)',
              lg: 'repeat(3, 1fr)',
            },
            gap: 2,
          }}
        >
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton
              key={i}
              variant="rounded"
              height={160}
              sx={{ borderRadius: '8px' }}
            />
          ))}
        </Box>
      ) : projects.length === 0 ? (
        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            py: 8,
          }}
        >
          <FolderOutlined sx={{ fontSize: 64, color: '#c3c6d7', mb: 2 }} />
          <Typography variant="subtitle1" color="text.secondary" sx={{ mb: 1 }}>
            暂无项目
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
            创建你的第一个项目，开始使用 Symphony 工作流
          </Typography>
          <Button
            variant="contained"
            startIcon={<Add />}
            onClick={() => navigate('/projects/new')}
          >
            创建项目
          </Button>
        </Box>
      ) : (
        <>
          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: {
                xs: '1fr',
                sm: 'repeat(2, 1fr)',
                lg: 'repeat(3, 1fr)',
              },
              gap: 2,
            }}
          >
            {projects.map((project) => (
              <ProjectCard
                key={project.id}
                project={project}
                onStart={handleStart}
                onStop={handleStop}
              />
            ))}
          </Box>

          {/* Pagination info */}
          {pagination.totalCount > 0 && (
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ mt: 2, textAlign: 'center' }}
            >
              共 {pagination.totalCount} 个项目
            </Typography>
          )}
        </>
      )}

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
          sx={{ borderRadius: '4px' }}
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}
