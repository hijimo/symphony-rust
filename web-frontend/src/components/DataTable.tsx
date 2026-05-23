import { ReactNode } from 'react';
import {
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TablePagination,
  Paper,
  Skeleton,
  Box,
  Typography,
} from '@mui/material';

export interface ColumnDef<T> {
  field: string;
  headerName: string;
  width?: number | string;
  align?: 'left' | 'center' | 'right';
  renderCell?: (row: T) => ReactNode;
}

interface DataTableProps<T> {
  columns: ColumnDef<T>[];
  data: T[];
  loading?: boolean;
  totalCount?: number;
  page?: number;
  pageSize?: number;
  onPageChange?: (page: number) => void;
  onPageSizeChange?: (size: number) => void;
  emptyMessage?: string;
  emptyIcon?: ReactNode;
}

export default function DataTable<T>({
  columns,
  data,
  loading = false,
  totalCount = 0,
  page = 0,
  pageSize = 10,
  onPageChange,
  onPageSizeChange,
  emptyMessage = '暂无数据',
  emptyIcon,
}: DataTableProps<T>) {
  if (loading) {
    return (
      <TableContainer
        component={Paper}
        variant="outlined"
        sx={{ borderRadius: 3 }}
      >
        <Table>
          <TableHead>
            <TableRow sx={{ bgcolor: 'grey.50' }}>
              {columns.map((col) => (
                <TableCell
                  key={col.field}
                  align={col.align}
                  sx={{ fontWeight: 600, width: col.width }}
                >
                  {col.headerName}
                </TableCell>
              ))}
            </TableRow>
          </TableHead>
          <TableBody>
            {Array.from({ length: 5 }).map((_, i) => (
              <TableRow key={i}>
                {columns.map((col) => (
                  <TableCell key={col.field} align={col.align}>
                    <Skeleton variant="text" />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    );
  }

  if (data.length === 0) {
    return (
      <TableContainer
        component={Paper}
        variant="outlined"
        sx={{ borderRadius: 3 }}
      >
        <Table>
          <TableHead>
            <TableRow sx={{ bgcolor: 'grey.50' }}>
              {columns.map((col) => (
                <TableCell
                  key={col.field}
                  align={col.align}
                  sx={{ fontWeight: 600, width: col.width }}
                >
                  {col.headerName}
                </TableCell>
              ))}
            </TableRow>
          </TableHead>
        </Table>
        <Box sx={{ py: 6, textAlign: 'center' }}>
          {emptyIcon && <Box sx={{ mb: 1.5, color: 'grey.300' }}>{emptyIcon}</Box>}
          <Typography variant="body1" color="text.secondary">
            {emptyMessage}
          </Typography>
        </Box>
      </TableContainer>
    );
  }

  return (
    <>
      <TableContainer
        component={Paper}
        variant="outlined"
        sx={{ borderRadius: 3 }}
      >
        <Table>
          <TableHead>
            <TableRow sx={{ bgcolor: 'grey.50' }}>
              {columns.map((col) => (
                <TableCell
                  key={col.field}
                  align={col.align}
                  sx={{ fontWeight: 600, width: col.width }}
                >
                  {col.headerName}
                </TableCell>
              ))}
            </TableRow>
          </TableHead>
          <TableBody>
            {data.map((row, idx) => (
              <TableRow key={idx} hover sx={{ '& td': { borderColor: 'grey.100' } }}>
                {columns.map((col) => (
                  <TableCell key={col.field} align={col.align}>
                    {col.renderCell
                      ? col.renderCell(row)
                      : (row as Record<string, unknown>)[col.field] as ReactNode}
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
      <TablePagination
        component="div"
        count={totalCount}
        page={page}
        rowsPerPage={pageSize}
        onPageChange={(_, newPage) => onPageChange?.(newPage)}
        onRowsPerPageChange={(e) => onPageSizeChange?.(parseInt(e.target.value, 10))}
        rowsPerPageOptions={[10, 25, 50]}
        labelRowsPerPage="每页"
        labelDisplayedRows={({ from, to, count }) => `第 ${from}-${to} 条，共 ${count} 条`}
      />
    </>
  );
}
