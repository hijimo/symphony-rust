import { useState, useEffect, useCallback } from 'react';
import { useParams } from 'react-router-dom';
import {
  Box,
  Typography,
  Button,
  Snackbar,
  Alert,
  Skeleton,
  Chip,
  CircularProgress,
} from '@mui/material';
import { Add, Sync, PeopleOutline } from '@mui/icons-material';
import { WarningAmber } from '@mui/icons-material';
import MemberTable from '../../components/MemberTable';
import AddMemberDialog from '../../components/AddMemberDialog';
import ConfirmDialog from '../../components/ConfirmDialog';
import { getMembers, addMember, updateMemberRole, removeMember, syncMembers } from '../../api/members';
import { getProject } from '../../api/projects';
import { useAuthStore } from '../../store/auth';
import type { Project, ProjectMember, ProjectMemberRole, SyncResult } from '../../types';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error' | 'info';
}

export default function ProjectMembersPage() {
  const { id } = useParams<{ id: string }>();
  const projectId = Number(id);
  const currentUser = useAuthStore((s) => s.user);

  const [loading, setLoading] = useState(true);
  const [project, setProject] = useState<Project | null>(null);
  const [members, setMembers] = useState<ProjectMember[]>([]);
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  // Add member dialog
  const [addOpen, setAddOpen] = useState(false);

  // Remove member dialog
  const [removeOpen, setRemoveOpen] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<ProjectMember | null>(null);
  const [removeLoading, setRemoveLoading] = useState(false);

  // Sync state
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<SyncResult | null>(null);

  const showSnack = (message: string, severity: 'success' | 'error' | 'info') => {
    setSnack({ open: true, message, severity });
  };

  const canManage =
    project?.my_role === 'owner' || project?.my_role === 'admin' || currentUser?.role === 'admin';

  const fetchData = useCallback(async () => {
    try {
      const [p, m] = await Promise.all([getProject(projectId), getMembers(projectId)]);
      setProject(p);
      setMembers(m);
    } catch (err: any) {
      showSnack(err?.message || '加载数据失败', 'error');
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  // Add member
  const handleAddMember = async (userId: number, role: ProjectMemberRole) => {
    try {
      await addMember(projectId, { user_id: userId, role });
      showSnack('成员已添加', 'success');
      const updated = await getMembers(projectId);
      setMembers(updated);
    } catch (err: any) {
      showSnack(err?.message || '添加成员失败', 'error');
      throw err;
    }
  };

  // Update role
  const handleRoleChange = async (userId: number, role: ProjectMemberRole) => {
    try {
      await updateMemberRole(projectId, userId, { role });
      setMembers((prev) =>
        prev.map((m) => (m.user_id === userId ? { ...m, role } : m)),
      );
      showSnack('角色已更新', 'success');
    } catch (err: any) {
      showSnack(err?.message || '更新角色失败', 'error');
    }
  };

  // Remove member
  const handleRemoveOpen = (member: ProjectMember) => {
    setRemoveTarget(member);
    setRemoveOpen(true);
  };

  const handleRemoveConfirm = async () => {
    if (!removeTarget) return;
    setRemoveLoading(true);
    try {
      await removeMember(projectId, removeTarget.user_id);
      setMembers((prev) => prev.filter((m) => m.user_id !== removeTarget.user_id));
      showSnack('成员已移除', 'success');
      setRemoveOpen(false);
    } catch (err: any) {
      showSnack(err?.message || '移除成员失败', 'error');
    } finally {
      setRemoveLoading(false);
    }
  };

  // Sync members
  const handleSync = async () => {
    setSyncing(true);
    setSyncResult(null);
    try {
      const result = await syncMembers(projectId);
      setSyncResult(result);
      showSnack(`同步完成: 新增 ${result.added} 人, 跳过 ${result.skipped} 人`, 'success');
      const updated = await getMembers(projectId);
      setMembers(updated);
    } catch (err: any) {
      showSnack(err?.message || '同步失败', 'error');
    } finally {
      setSyncing(false);
    }
  };

  if (loading) {
    return (
      <Box>
        <Skeleton variant="text" width={200} height={40} sx={{ mb: 3 }} />
        <Skeleton variant="rounded" height={400} sx={{ borderRadius: '8px' }} />
      </Box>
    );
  }

  if (!project) {
    return (
      <Box sx={{ textAlign: 'center', py: 6 }}>
        <PeopleOutline sx={{ fontSize: 64, color: '#c3c6d7', mb: 2 }} />
        <Typography variant="h6" color="text.secondary">
          项目不存在或无权访问
        </Typography>
      </Box>
    );
  }

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 3 }}>
        <Typography variant="h5" color="text.primary">
          项目成员 - {project.name}
        </Typography>
        {canManage && (
          <Box sx={{ display: 'flex', gap: 1.5 }}>
            <Button
              variant="outlined"
              startIcon={syncing ? <CircularProgress size={16} color="inherit" /> : <Sync />}
              onClick={handleSync}
              disabled={syncing}
            >
              从平台同步
            </Button>
            <Button
              variant="contained"
              startIcon={<Add />}
              onClick={() => setAddOpen(true)}
            >
              添加成员
            </Button>
          </Box>
        )}
      </Box>

      {/* Sync Result */}
      {syncResult && (
        <Box sx={{ mb: 2, p: 2, bgcolor: '#f3f3fe', borderRadius: '8px' }}>
          <Typography variant="subtitle2" sx={{ mb: 1 }}>
            同步结果
          </Typography>
          <Box sx={{ display: 'flex', gap: 2, flexWrap: 'wrap', alignItems: 'center' }}>
            <Chip label={`新增 ${syncResult.added} 人`} color="success" size="small" variant="outlined" />
            <Chip label={`跳过 ${syncResult.skipped} 人`} size="small" variant="outlined" />
            {syncResult.unmatched.length > 0 && (
              <Box>
                <Typography variant="body2" color="text.secondary" component="span">
                  未匹配: {syncResult.unmatched.join(', ')}
                </Typography>
              </Box>
            )}
          </Box>
        </Box>
      )}

      {/* Members Table */}
      <MemberTable
        members={members}
        loading={false}
        currentUserId={currentUser?.id ?? 0}
        canManage={!!canManage}
        onRoleChange={handleRoleChange}
        onRemove={handleRemoveOpen}
      />

      {/* Add Member Dialog */}
      <AddMemberDialog
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onAdd={handleAddMember}
        existingMemberIds={members.map((m) => m.user_id)}
      />

      {/* Remove Confirm Dialog */}
      <ConfirmDialog
        open={removeOpen}
        title="移除成员"
        message={
          <>
            确定要将 "{removeTarget?.display_name || removeTarget?.username}" 从项目中移除吗？
            <br />
            <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
              移除后该用户将无法访问此项目。
            </Typography>
          </>
        }
        confirmText="确认移除"
        confirmColor="error"
        icon={<WarningAmber sx={{ fontSize: 48, color: 'warning.main' }} />}
        loading={removeLoading}
        onConfirm={handleRemoveConfirm}
        onCancel={() => setRemoveOpen(false)}
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
