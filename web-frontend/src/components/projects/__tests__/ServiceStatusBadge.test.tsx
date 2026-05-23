import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import ServiceStatusBadge from '../ServiceStatusBadge';
import type { ServiceStatus } from '../../../types';

function renderBadge(status: ServiceStatus) {
  return render(
    <ThemeProvider theme={theme}>
      <ServiceStatusBadge status={status} />
    </ThemeProvider>,
  );
}

describe('ServiceStatusBadge', () => {
  it('renders "运行中" with green color for running status', () => {
    renderBadge('running');
    const badge = screen.getByText('运行中');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#1b6e2d' });
  });

  it('renders "已停止" with gray color for stopped status', () => {
    renderBadge('stopped');
    const badge = screen.getByText('已停止');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#434655' });
  });

  it('renders "异常" with red color for error status', () => {
    renderBadge('error');
    const badge = screen.getByText('异常');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#93000a' });
  });

  it('renders "失败" with red color for failed status', () => {
    renderBadge('failed');
    const badge = screen.getByText('失败');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#93000a' });
  });

  it('renders "启动中" with yellow color for starting status', () => {
    renderBadge('starting');
    const badge = screen.getByText('启动中');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#7c5800' });
  });

  it('renders "停止中" with yellow color for stopping status', () => {
    renderBadge('stopping');
    const badge = screen.getByText('停止中');
    expect(badge).toBeInTheDocument();
    expect(badge.closest('.MuiChip-root')).toHaveStyle({ color: '#7c5800' });
  });

  it('has correct aria-label for accessibility', () => {
    renderBadge('running');
    expect(screen.getByLabelText('服务状态：运行中')).toBeInTheDocument();
  });
});
