import { useState } from 'react';
import {
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TablePagination,
  Box,
  Typography,
  Collapse,
  IconButton,
  Chip,
  Skeleton,
} from '@mui/material';
import KeyboardArrowDownIcon from '@mui/icons-material/KeyboardArrowDown';
import KeyboardArrowUpIcon from '@mui/icons-material/KeyboardArrowUp';
import type { AlertHistoryRecord } from '../../types/alert';
import type { PaginationData } from '../../types';
import SeverityBadge from './SeverityBadge';

const statusConfig: Record<string, { label: string; color: 'success' | 'error' | 'default' }> = {
  sent: { label: '已发送', color: 'success' },
  failed: { label: '发送失败', color: 'error' },
  suppressed: { label: '已抑制', color: 'default' },
  pending: { label: '待发送', color: 'default' },
};

interface AlertHistoryTableProps {
  data: PaginationData<AlertHistoryRecord> | null;
  loading: boolean;
  onPageChange: (page: number, pageSize: number) => void;
}

function ExpandableRow({ record }: { record: AlertHistoryRecord }) {
  const [open, setOpen] = useState(false);
  const status = record.notificationStatus
    ? statusConfig[record.notificationStatus] ?? statusConfig.pending
    : null;

  return (
    <>
      <TableRow
        hover
        onClick={() => setOpen(!open)}
        sx={{ cursor: 'pointer', '& > *': { borderBottom: open ? 'none' : undefined } }}
      >
        <TableCell sx={{ width: 40, p: 0.5 }}>
          <IconButton size="small" aria-label={open ? '收起详情' : '展开详情'}>
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </IconButton>
        </TableCell>
        <TableCell>
          <SeverityBadge severity={record.severity} />
        </TableCell>
        <TableCell>
          <Typography variant="body2" sx={{ fontWeight: 500, fontSize: '14px' }}>
            {record.title}
          </Typography>
        </TableCell>
        <TableCell>
          <Typography variant="body2" sx={{ fontSize: '14px', color: '#434655' }}>
            {record.projectName ?? '系统'}
          </Typography>
        </TableCell>
        <TableCell>
          <Typography variant="body2" sx={{ fontSize: '14px', color: '#434655' }}>
            {formatTime(record.firedAt)}
          </Typography>
        </TableCell>
        <TableCell>
          {status && (
            <Chip
              label={status.label}
              size="small"
              color={status.color}
              variant="outlined"
              sx={{ borderRadius: '4px', fontSize: '12px' }}
            />
          )}
        </TableCell>
      </TableRow>
      <TableRow>
        <TableCell colSpan={6} sx={{ py: 0, px: 0 }}>
          <Collapse in={open} timeout="auto" unmountOnExit>
            <Box sx={{ p: 2, pl: 7, bgcolor: '#f3f3fe', borderRadius: 1, m: 1 }}>
              <Typography variant="body2" sx={{ mb: 1, color: '#191b23' }}>
                {record.message}
              </Typography>
              {record.context && Object.keys(record.context).length > 0 && (
                <Box sx={{ mt: 1 }}>
                  <Typography
                    variant="caption"
                    sx={{ fontWeight: 500, color: '#737686', mb: 0.5, display: 'block' }}
                  >
                    上下文
                  </Typography>
                  <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 1 }}>
                    {Object.entries(record.context).map(([key, value]) => (
                      <Chip
                        key={key}
                        label={`${key}: ${value}`}
                        size="small"
                        variant="outlined"
                        sx={{ borderRadius: '4px', fontSize: '11px' }}
                      />
                    ))}
                  </Box>
                </Box>
              )}
              {record.notifiedAt && (
                <Typography variant="caption" sx={{ mt: 1, display: 'block', color: '#737686' }}>
                  通知时间: {formatTime(record.notifiedAt)}
                  {record.notificationChannel && ` (${record.notificationChannel})`}
                </Typography>
              )}
            </Box>
          </Collapse>
        </TableCell>
      </TableRow>
    </>
  );
}

export default function AlertHistoryTable({ data, loading, onPageChange }: AlertHistoryTableProps) {
  if (loading && !data) {
    return (
      <Box>
        {[...Array(5)].map((_, i) => (
          <Skeleton key={i} height={48} sx={{ mb: 0.5 }} />
        ))}
      </Box>
    );
  }

  if (!data || data.records.length === 0) {
    return (
      <Box sx={{ py: 6, textAlign: 'center' }}>
        <Typography color="text.secondary">暂无告警记录</Typography>
      </Box>
    );
  }

  return (
    <Box>
      <TableContainer>
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell sx={{ width: 40 }} />
              <TableCell sx={{ fontWeight: 500, fontSize: '12px', color: '#737686' }}>
                严重级别
              </TableCell>
              <TableCell sx={{ fontWeight: 500, fontSize: '12px', color: '#737686' }}>
                标题
              </TableCell>
              <TableCell sx={{ fontWeight: 500, fontSize: '12px', color: '#737686' }}>
                项目
              </TableCell>
              <TableCell sx={{ fontWeight: 500, fontSize: '12px', color: '#737686' }}>
                触发时间
              </TableCell>
              <TableCell sx={{ fontWeight: 500, fontSize: '12px', color: '#737686' }}>
                通知状态
              </TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {data.records.map((record) => (
              <ExpandableRow key={record.id} record={record} />
            ))}
          </TableBody>
        </Table>
      </TableContainer>
      <TablePagination
        component="div"
        count={data.totalCount}
        page={data.pageNo - 1}
        rowsPerPage={data.pageSize}
        onPageChange={(_, page) => onPageChange(page + 1, data.pageSize)}
        onRowsPerPageChange={(e) => onPageChange(1, parseInt(e.target.value, 10))}
        rowsPerPageOptions={[10, 20, 50]}
        labelRowsPerPage="每页条数"
        labelDisplayedRows={({ from, to, count }) => `${from}-${to} / ${count}`}
      />
    </Box>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString('zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return iso;
  }
}
