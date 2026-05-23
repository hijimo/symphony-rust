import {
  Box,
  Chip,
  IconButton,
  Select,
  MenuItem,
  Tooltip,
  Typography,
} from '@mui/material';
import { Delete, PeopleOutline } from '@mui/icons-material';
import DataTable from './DataTable';
import type { ColumnDef } from './DataTable';
import type { ProjectMember, ProjectMemberRole } from '../types';

interface MemberTableProps {
  members: ProjectMember[];
  loading: boolean;
  currentUserId: number;
  canManage: boolean;
  onRoleChange: (userId: number, role: ProjectMemberRole) => void;
  onRemove: (member: ProjectMember) => void;
}

export default function MemberTable({
  members,
  loading,
  currentUserId,
  canManage,
  onRoleChange,
  onRemove,
}: MemberTableProps) {
  const columns: ColumnDef<ProjectMember>[] = [
    {
      field: 'username',
      headerName: '用户名',
      width: 140,
    },
    {
      field: 'display_name',
      headerName: '显示名',
      width: 140,
      renderCell: (row) => row.display_name || '-',
    },
    {
      field: 'role',
      headerName: '角色',
      width: 140,
      renderCell: (row) => {
        if (!canManage || row.user_id === currentUserId) {
          return (
            <Chip
              label={row.role}
              size="small"
              color={row.role === 'owner' ? 'primary' : 'default'}
              variant={row.role === 'owner' ? 'filled' : 'outlined'}
            />
          );
        }
        return (
          <Select
            value={row.role}
            size="small"
            onChange={(e) => onRoleChange(row.user_id, e.target.value as ProjectMemberRole)}
            sx={{ minWidth: 100, height: 32 }}
            aria-label={`修改 ${row.display_name || row.username} 的角色`}
          >
            <MenuItem value="owner">owner</MenuItem>
            <MenuItem value="member">member</MenuItem>
          </Select>
        );
      },
    },
    {
      field: 'synced_from',
      headerName: '来源',
      width: 100,
      align: 'center',
      renderCell: (row) =>
        row.synced_from ? (
          <Chip label={row.synced_from} size="small" variant="outlined" />
        ) : (
          <Typography variant="body2" color="text.secondary">手动</Typography>
        ),
    },
    {
      field: 'created_at',
      headerName: '加入时间',
      width: 160,
      renderCell: (row) => {
        const d = new Date(row.created_at);
        return d.toLocaleString('zh-CN', {
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
        });
      },
    },
    ...(canManage
      ? [
          {
            field: 'actions',
            headerName: '操作',
            width: 80,
            align: 'center' as const,
            renderCell: (row: ProjectMember) => {
              if (row.user_id === currentUserId) return null;
              return (
                <Tooltip title="移除成员">
                  <IconButton
                    size="small"
                    color="error"
                    onClick={() => onRemove(row)}
                    aria-label={`移除成员 ${row.display_name || row.username}`}
                  >
                    <Delete fontSize="small" />
                  </IconButton>
                </Tooltip>
              );
            },
          },
        ]
      : []),
  ];

  return (
    <Box>
      <DataTable
        columns={columns}
        data={members}
        loading={loading}
        totalCount={members.length}
        page={0}
        pageSize={members.length || 10}
        emptyMessage="暂无成员"
        emptyIcon={<PeopleOutline sx={{ fontSize: 64, color: '#c3c6d7' }} />}
      />
    </Box>
  );
}
