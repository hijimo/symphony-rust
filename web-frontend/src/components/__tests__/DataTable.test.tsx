import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../theme';
import DataTable from '../DataTable';
import type { ColumnDef } from '../DataTable';

interface TestRow {
  id: number;
  name: string;
}

const columns: ColumnDef<TestRow>[] = [
  { field: 'id', headerName: 'ID' },
  { field: 'name', headerName: '名称' },
];

const testData: TestRow[] = [
  { id: 1, name: 'Alice' },
  { id: 2, name: 'Bob' },
];

function renderTable(props = {}) {
  return render(
    <ThemeProvider theme={theme}>
      <DataTable columns={columns} data={testData} totalCount={2} {...props} />
    </ThemeProvider>,
  );
}

describe('DataTable', () => {
  it('renders column headers and data rows', () => {
    renderTable();
    expect(screen.getByText('ID')).toBeInTheDocument();
    expect(screen.getByText('名称')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('Bob')).toBeInTheDocument();
  });

  it('shows skeleton loading state', () => {
    renderTable({ loading: true });
    expect(screen.getByText('ID')).toBeInTheDocument();
    expect(screen.queryByText('Alice')).not.toBeInTheDocument();
  });

  it('shows empty state when data is empty', () => {
    renderTable({ data: [], totalCount: 0 });
    expect(screen.getByText('暂无数据')).toBeInTheDocument();
  });

  it('shows custom empty message', () => {
    renderTable({ data: [], totalCount: 0, emptyMessage: '没有记录' });
    expect(screen.getByText('没有记录')).toBeInTheDocument();
  });

  it('triggers page change callback', async () => {
    const user = userEvent.setup();
    const onPageChange = vi.fn();
    renderTable({ totalCount: 30, page: 0, pageSize: 10, onPageChange });

    const nextBtn = screen.getByLabelText('Go to next page');
    await user.click(nextBtn);
    expect(onPageChange).toHaveBeenCalledWith(1);
  });

  it('triggers page size change callback', async () => {
    const user = userEvent.setup();
    const onPageSizeChange = vi.fn();
    renderTable({ totalCount: 30, page: 0, pageSize: 10, onPageSizeChange });

    const select = screen.getByRole('combobox');
    await user.click(select);
    const option25 = screen.getByRole('option', { name: '25' });
    await user.click(option25);
    expect(onPageSizeChange).toHaveBeenCalledWith(25);
  });
});
