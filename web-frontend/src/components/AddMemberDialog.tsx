import { useState, useEffect } from 'react';
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  TextField,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  CircularProgress,
  InputAdornment,
  Box,
  Typography,
} from '@mui/material';
import { Search } from '@mui/icons-material';
import type { UserProfile, ProjectMemberRole } from '../types';
import { getUsers } from '../api/admin';

interface AddMemberDialogProps {
  open: boolean;
  onClose: () => void;
  onAdd: (userId: number, role: ProjectMemberRole) => Promise<void>;
  existingMemberIds: number[];
}

export default function AddMemberDialog({
  open,
  onClose,
  onAdd,
  existingMemberIds,
}: AddMemberDialogProps) {
  const [search, setSearch] = useState('');
  const [users, setUsers] = useState<UserProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedUser, setSelectedUser] = useState<UserProfile | null>(null);
  const [role, setRole] = useState<ProjectMemberRole>('member');
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open) {
      setSearch('');
      setUsers([]);
      setSelectedUser(null);
      setRole('member');
      return;
    }
  }, [open]);

  useEffect(() => {
    if (!search.trim()) {
      setUsers([]);
      return;
    }
    const timer = setTimeout(async () => {
      setLoading(true);
      try {
        const data = await getUsers({ pageNo: 1, pageSize: 20, search: search.trim() });
        setUsers(data.records.filter((u) => !existingMemberIds.includes(u.id)));
      } catch {
        setUsers([]);
      } finally {
        setLoading(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [search, existingMemberIds]);

  const handleSubmit = async () => {
    if (!selectedUser) return;
    setSubmitting(true);
    try {
      await onAdd(selectedUser.id, role);
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 600 }}>添加成员</DialogTitle>
      <DialogContent sx={{ display: 'flex', flexDirection: 'column', gap: 2.5, pt: '16px !important' }}>
        <TextField
          fullWidth
          placeholder="搜索用户名或显示名..."
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setSelectedUser(null);
          }}
          variant="filled"
          size="small"
          slotProps={{
            input: {
              startAdornment: (
                <InputAdornment position="start">
                  <Search color="action" />
                </InputAdornment>
              ),
            },
          }}
          aria-label="搜索用户"
        />

        {/* User list */}
        {loading && (
          <Box sx={{ display: 'flex', justifyContent: 'center', py: 2 }}>
            <CircularProgress size={24} />
          </Box>
        )}
        {!loading && users.length > 0 && (
          <Box
            sx={{
              maxHeight: 200,
              overflow: 'auto',
              border: '1px solid #c3c6d7',
              borderRadius: '4px',
            }}
          >
            {users.map((user) => (
              <Box
                key={user.id}
                onClick={() => setSelectedUser(user)}
                sx={{
                  px: 2,
                  py: 1.5,
                  cursor: 'pointer',
                  bgcolor: selectedUser?.id === user.id ? 'rgba(0, 62, 168, 0.08)' : 'transparent',
                  '&:hover': { bgcolor: 'rgba(0, 62, 168, 0.04)' },
                  borderBottom: '1px solid #f0f0f4',
                  '&:last-child': { borderBottom: 'none' },
                }}
                role="option"
                aria-selected={selectedUser?.id === user.id}
              >
                <Typography variant="subtitle2">{user.displayName || user.username}</Typography>
                <Typography variant="body2" color="text.secondary">
                  @{user.username}
                </Typography>
              </Box>
            ))}
          </Box>
        )}
        {!loading && search.trim() && users.length === 0 && (
          <Typography variant="body2" color="text.secondary" sx={{ textAlign: 'center', py: 2 }}>
            未找到可添加的用户
          </Typography>
        )}

        {selectedUser && (
          <Box sx={{ p: 1.5, bgcolor: '#f3f3fe', borderRadius: '4px' }}>
            <Typography variant="subtitle2">
              已选择: {selectedUser.displayName || selectedUser.username} (@{selectedUser.username})
            </Typography>
          </Box>
        )}

        <FormControl fullWidth size="small">
          <InputLabel>角色</InputLabel>
          <Select
            value={role}
            label="角色"
            onChange={(e) => setRole(e.target.value as ProjectMemberRole)}
          >
            <MenuItem value="member">member</MenuItem>
            <MenuItem value="owner">owner</MenuItem>
          </Select>
        </FormControl>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2.5 }}>
        <Button onClick={onClose} color="inherit" disabled={submitting}>
          取消
        </Button>
        <Button
          onClick={handleSubmit}
          variant="contained"
          disabled={!selectedUser || submitting}
          startIcon={submitting ? <CircularProgress size={16} color="inherit" /> : undefined}
        >
          确认添加
        </Button>
      </DialogActions>
    </Dialog>
  );
}
