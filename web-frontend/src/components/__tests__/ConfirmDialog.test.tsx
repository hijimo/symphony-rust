import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../theme';
import ConfirmDialog from '../ConfirmDialog';

function renderDialog(props = {}) {
  const defaultProps = {
    open: true,
    title: '确认操作',
    message: '确定要执行此操作吗？',
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
    ...props,
  };
  return {
    ...render(
      <ThemeProvider theme={theme}>
        <ConfirmDialog {...defaultProps} />
      </ThemeProvider>,
    ),
    props: defaultProps,
  };
}

describe('ConfirmDialog', () => {
  it('displays title and message when open', () => {
    renderDialog();
    expect(screen.getByText('确认操作')).toBeInTheDocument();
    expect(screen.getByText('确定要执行此操作吗？')).toBeInTheDocument();
  });

  it('does not render content when closed', () => {
    renderDialog({ open: false });
    expect(screen.queryByText('确认操作')).not.toBeInTheDocument();
  });

  it('calls onCancel when cancel button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderDialog();

    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it('calls onConfirm when confirm button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderDialog();

    await user.click(screen.getByRole('button', { name: '确认' }));
    expect(props.onConfirm).toHaveBeenCalledTimes(1);
  });

  it('disables buttons when loading', () => {
    renderDialog({ loading: true });
    expect(screen.getByRole('button', { name: /确认/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: '取消' })).toBeDisabled();
  });

  it('uses custom button text', () => {
    renderDialog({ confirmText: '删除', cancelText: '返回' });
    expect(screen.getByRole('button', { name: '删除' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '返回' })).toBeInTheDocument();
  });
});
