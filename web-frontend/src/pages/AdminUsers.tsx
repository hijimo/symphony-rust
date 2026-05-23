import { useState, useEffect, useCallback } from 'react';
import {
  Box,
  Paper,
  Typography,
  Button,
  TextField,
  Select,
  MenuItem,
  IconButton,
  Tooltip,
  Chip,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Snackbar,
  Alert,
  InputAdornment,
  FormControl,
  InputLabel,
  CircularProgress,
} from '@mui/material';
import {
  Add,
  Search,
  Refresh,
  LockReset,
  Delete,
  WarningAmber,
  PeopleOutline,
} from '@mui/icons-material';
import DataTable from '../components/DataTable';
import type { ColumnDef } from '../components/DataTable';
import PasswordField from '../components/PasswordField';
import ConfirmDialog from '../components/ConfirmDialog';
import { getUsers, createUser, deleteUser, resetPassword } from '../api/admin';
import type { UserProfile, UserRole } from '../types';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

export default function AdminUsers() {
  const [users, setUsers] = useState<UserProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [totalCount, setTotalCount] = useState(0);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(10);
  const [search, setSearch] = useState('');
  const [roleFilter, setRoleFilter] = useState('');
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  // Add user dialog
  const [addOpen, setAddOpen] = useState(false);
  const [addLoading, setAddLoading] = useState(false);
  const [addForm, setAddForm] = useState({ username: '', password: '', displayName: '', role: 'user' as UserRole });
  const [addErrors, setAddErrors] = useState<Record<string, string>>({});

  // Reset password dialog
  const [resetOpen, setResetOpen] = useState(false);
  const [resetLoading, setResetLoading] = useState(false);
  const [resetTarget, setResetTarget] = useState<UserProfile | null>(null);
  const [resetForm, setResetForm] = useState({ newPassword: '', confirmPassword: '' });
  const [resetErrors, setResetErrors] = useState<Record<string, string>>({});

  // Delete dialog
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteLoading, setDeleteLoading] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<UserProfile | null>(null);

  const fetchUsers = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getUsers({
        pageNo: page + 1,
        pageSize,
        search: search || undefined,
        role: roleFilter || undefined,
      });
      setUsers(data.records);
      setTotalCount(data.totalCount);
    } catch (err: any) {
      setSnack({ open: true, message: err?.message || '获取用户列表失败', severity: 'error' });
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, search, roleFilter]);

  useEffect(() => {
    fetchUsers();
  }, [fetchUsers]);

  // Debounced search
  const [searchInput, setSearchInput] = useState('');
  useEffect(() => {
    const timer = setTimeout(() => {
      setSearch(searchInput);
      setPage(0);
    }, 300);
    return () => clearTimeout(timer);
  }, [searchInput]);

  const showSnack = (message: string, severity: 'success' | 'error') => {
    setSnack({ open: true, message, severity });
  };

  // Add user handlers
  const handleAddOpen = () => {
    setAddForm({ username: '', password: '', displayName: '', role: 'user' });
    setAddErrors({});
    setAddOpen(true);
  };

  const validateAddForm = () => {
    const errors: Record<string, string> = {};
    if (!addForm.username || addForm.username.length < 3 || addForm.username.length > 32) {
      errors.username = '用户名需要3-32个字符';
    } else if (!/^[a-zA-Z0-9_]+$/.test(addForm.username)) {
      errors.username = '仅允许字母、数字和下划线';
    }
    if (!addForm.password || addForm.password.length < 6) {
      errors.password = '密码至少6个字符';
    }
    if (addForm.displayName && addForm.displayName.length > 64) {
      errors.displayName = '显示名最多64个字符';
    }
    setAddErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const handleAddSubmit = async () => {
    if (!validateAddForm()) return;
    setAddLoading(true);
    try {
      await createUser({
        username: addForm.username,
        password: addForm.password,
        displayName: addForm.displayName || undefined,
        role: addForm.role,
      });
      showSnack('用户创建成功', 'success');
      setAddOpen(false);
      fetchUsers();
    } catch (err: any) {
      showSnack(err?.message || '创建用户失败', 'error');
    } finally {
      setAddLoading(false);
    }
  };

  // Reset password handlers
  const handleResetOpen = (user: UserProfile) => {
    setResetTarget(user);
    setResetForm({ newPassword: '', confirmPassword: '' });
    setResetErrors({});
    setResetOpen(true);
  };

  const validateResetForm = () => {
    const errors: Record<string, string> = {};
    if (!resetForm.newPassword || resetForm.newPassword.length < 6) {
      errors.newPassword = '密码至少6个字符';
    }
    if (resetForm.newPassword !== resetForm.confirmPassword) {
      errors.confirmPassword = '两次输入的密码不一致';
    }
    setResetErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const handleResetSubmit = async () => {
    if (!validateResetForm() || !resetTarget) return;
    setResetLoading(true);
    try {
      await resetPassword(resetTarget.id, { newPassword: resetForm.newPassword });
      showSnack('密码重置成功', 'success');
      setResetOpen(false);
    } catch (err: any) {
      showSnack(err?.message || '重置密码失败', 'error');
    } finally {
      setResetLoading(false);
    }
  };

  // Delete handlers
  const handleDeleteOpen = (user: UserProfile) => {
    setDeleteTarget(user);
    setDeleteOpen(true);
  };

  const handleDeleteConfirm = async () => {
    if (!deleteTarget) return;
    setDeleteLoading(true);
    try {
      await deleteUser(deleteTarget.id);
      showSnack('用户已删除', 'success');
      setDeleteOpen(false);
      fetchUsers();
    } catch (err: any) {
      showSnack(err?.message || '删除用户失败', 'error');
    } finally {
      setDeleteLoading(false);
    }
  };

  const columns: ColumnDef<UserProfile>[] = [
    { field: 'id', headerName: 'ID', width: 60, align: 'center' },
    { field: 'username', headerName: '用户名', width: 160 },
    {
      field: 'displayName',
      headerName: '显示名',
      width: 160,
      renderCell: (row) => row.displayName || '-',
    },
    {
      field: 'role',
      headerName: '角色',
      width: 100,
      align: 'center',
      renderCell: (row) => (
        <Chip
          label={row.role}
          size="small"
          color={row.role === 'admin' ? 'primary' : 'default'}
          variant={row.role === 'admin' ? 'filled' : 'outlined'}
        />
      ),
    },
    {
      field: 'createdAt',
      headerName: '创建时间',
      width: 160,
      renderCell: (row) => {
        const d = new Date(row.createdAt);
        return d.toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
      },
    },
    {
      field: 'actions',
      headerName: '操作',
      width: 140,
      align: 'center',
      renderCell: (row) => (
        <Box>
          <Tooltip title="重置密码">
            <IconButton
              size="small"
              onClick={() => handleResetOpen(row)}
              aria-label={`重置用户${row.displayName || row.username}的密码`}
            >
              <LockReset fontSize="small" />
            </IconButton>
          </Tooltip>
          <Tooltip title="删除用户">
            <IconButton
              size="small"
              color="error"
              onClick={() => handleDeleteOpen(row)}
              aria-label={`删除用户${row.displayName || row.username}`}
            >
              <Delete fontSize="small" />
            </IconButton>
          </Tooltip>
        </Box>
      ),
    },
  ];

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 3 }}>
        <Typography variant="h5" color="text.primary">
          用户管理
        </Typography>
        <Button
          variant="contained"
          startIcon={<Add />}
          onClick={handleAddOpen}
        >
          添加用户
        </Button>
      </Box>

      {/* Toolbar */}
      <Paper
        elevation={0}
        sx={{
          display: 'flex',
          gap: 1.5,
          alignItems: 'center',
          mb: 2,
          flexWrap: 'wrap',
          p: 0,
          backgroundColor: 'transparent',
        }}
      >
        <TextField
          size="small"
          placeholder="搜索用户名或显示名..."
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          sx={{ width: { xs: '100%', sm: 280 } }}
          aria-label="搜索用户"
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
          <InputLabel>角色筛选</InputLabel>
          <Select
            value={roleFilter}
            label="角色筛选"
            onChange={(e) => {
              setRoleFilter(e.target.value);
              setPage(0);
            }}
          >
            <MenuItem value="">全部角色</MenuItem>
            <MenuItem value="admin">admin</MenuItem>
            <MenuItem value="user">user</MenuItem>
          </Select>
        </FormControl>
        <Tooltip title="刷新">
          <IconButton onClick={fetchUsers} aria-label="刷新列表">
            <Refresh />
          </IconButton>
        </Tooltip>
      </Paper>

      {/* Table */}
      <DataTable
        columns={columns}
        data={users}
        loading={loading}
        totalCount={totalCount}
        page={page}
        pageSize={pageSize}
        onPageChange={setPage}
        onPageSizeChange={(size) => {
          setPageSize(size);
          setPage(0);
        }}
        emptyMessage="暂无用户数据"
        emptyIcon={<PeopleOutline sx={{ fontSize: 64, color: '#c3c6d7' }} />}
      />

      {/* Add User Dialog */}
      <Dialog
        open={addOpen}
        onClose={() => setAddOpen(false)}
        maxWidth="xs"
        fullWidth
      >
        <DialogTitle sx={{ fontWeight: 600 }}>添加用户</DialogTitle>
        <DialogContent sx={{ display: 'flex', flexDirection: 'column', gap: 2.5, pt: '16px !important' }}>
          <TextField
            label="用户名"
            required
            fullWidth
            value={addForm.username}
            onChange={(e) => setAddForm((f) => ({ ...f, username: e.target.value }))}
            error={!!addErrors.username}
            helperText={addErrors.username}
            placeholder="请输入用户名"
          />
          <PasswordField
            label="密码"
            required
            fullWidth
            value={addForm.password}
            onChange={(e) => setAddForm((f) => ({ ...f, password: e.target.value }))}
            error={!!addErrors.password}
            helperText={addErrors.password}
            placeholder="请输入密码"
          />
          <TextField
            label="显示名"
            fullWidth
            value={addForm.displayName}
            onChange={(e) => setAddForm((f) => ({ ...f, displayName: e.target.value }))}
            error={!!addErrors.displayName}
            helperText={addErrors.displayName}
            placeholder="请输入显示名"
          />
          <FormControl fullWidth required>
            <InputLabel>角色</InputLabel>
            <Select
              value={addForm.role}
              label="角色"
              onChange={(e) => setAddForm((f) => ({ ...f, role: e.target.value as UserRole }))}
            >
              <MenuItem value="user">user</MenuItem>
              <MenuItem value="admin">admin</MenuItem>
            </Select>
          </FormControl>
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2.5 }}>
          <Button onClick={() => setAddOpen(false)} color="inherit" disabled={addLoading}>
            取消
          </Button>
          <Button
            onClick={handleAddSubmit}
            variant="contained"
            disabled={addLoading}
            startIcon={addLoading ? <CircularProgress size={16} color="inherit" /> : undefined}
          >
            确认添加
          </Button>
        </DialogActions>
      </Dialog>

      {/* Reset Password Dialog */}
      <Dialog
        open={resetOpen}
        onClose={() => setResetOpen(false)}
        maxWidth="xs"
        fullWidth
      >
        <DialogTitle sx={{ fontWeight: 600 }}>重置密码</DialogTitle>
        <DialogContent sx={{ display: 'flex', flexDirection: 'column', gap: 2.5, pt: '16px !important' }}>
          <Typography variant="body2" color="text.secondary">
            确定要重置用户 "{resetTarget?.displayName || resetTarget?.username}" 的密码？
          </Typography>
          <PasswordField
            label="新密码"
            required
            fullWidth
            value={resetForm.newPassword}
            onChange={(e) => setResetForm((f) => ({ ...f, newPassword: e.target.value }))}
            error={!!resetErrors.newPassword}
            helperText={resetErrors.newPassword}
            placeholder="请输入新密码"
          />
          <PasswordField
            label="确认密码"
            required
            fullWidth
            value={resetForm.confirmPassword}
            onChange={(e) => setResetForm((f) => ({ ...f, confirmPassword: e.target.value }))}
            error={!!resetErrors.confirmPassword}
            helperText={resetErrors.confirmPassword}
            placeholder="请再次输入新密码"
          />
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2.5 }}>
          <Button onClick={() => setResetOpen(false)} color="inherit" disabled={resetLoading}>
            取消
          </Button>
          <Button
            onClick={handleResetSubmit}
            variant="contained"
            disabled={resetLoading}
            startIcon={resetLoading ? <CircularProgress size={16} color="inherit" /> : undefined}
          >
            确认重置
          </Button>
        </DialogActions>
      </Dialog>

      {/* Delete Confirm Dialog */}
      <ConfirmDialog
        open={deleteOpen}
        title="删除用户"
        message={
          <>
            确定要删除用户 "{deleteTarget?.displayName || deleteTarget?.username}" 吗？
            <br />
            <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
              此操作不可撤销，该用户的所有数据将被永久删除。
            </Typography>
          </>
        }
        confirmText="确认删除"
        confirmColor="error"
        icon={<WarningAmber sx={{ fontSize: 48, color: 'warning.main' }} />}
        loading={deleteLoading}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteOpen(false)}
      />

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
