import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import ServiceControlButton from '../ServiceControlButton';
import type { ServiceStatus } from '../../../types';

function renderButton(
  status: ServiceStatus,
  overrides: Partial<{
    onStart: () => Promise<void>;
    onStop: () => Promise<void>;
    onRestart: () => Promise<void>;
  }> = {},
) {
  const props = {
    status,
    onStart: vi.fn().mockResolvedValue(undefined),
    onStop: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  return {
    ...render(
      <ThemeProvider theme={theme}>
        <ServiceControlButton {...props} />
      </ThemeProvider>,
    ),
    props,
  };
}

describe('ServiceControlButton', () => {
  it('shows start button when stopped', () => {
    renderButton('stopped');
    expect(screen.getByLabelText('启动服务')).toBeInTheDocument();
  });

  it('shows stop button when running', () => {
    renderButton('running');
    expect(screen.getByLabelText('停止服务')).toBeInTheDocument();
  });

  it('shows start button when status is error', () => {
    renderButton('error');
    expect(screen.getByLabelText('启动服务')).toBeInTheDocument();
  });

  it('shows start button when status is failed', () => {
    renderButton('failed');
    expect(screen.getByLabelText('启动服务')).toBeInTheDocument();
  });

  it('shows loading state during transitioning status (starting)', () => {
    renderButton('starting');
    const button = screen.getByRole('button');
    expect(button).toBeDisabled();
  });

  it('shows loading state during transitioning status (stopping)', () => {
    renderButton('stopping');
    const button = screen.getByRole('button');
    expect(button).toBeDisabled();
  });

  it('shows loading state during action execution', async () => {
    const user = userEvent.setup();
    let resolveAction: () => void;
    const onStart = () =>
      new Promise<void>((resolve) => {
        resolveAction = resolve;
      });

    renderButton('stopped', { onStart });

    await user.click(screen.getByLabelText('启动服务'));

    // Should show loading (disabled button)
    await waitFor(() => {
      const buttons = screen.getAllByRole('button');
      expect(buttons[0]).toBeDisabled();
    });

    // Resolve the action
    resolveAction!();

    // Should return to normal
    await waitFor(() => {
      expect(screen.getByLabelText('启动服务')).toBeInTheDocument();
    });
  });

  it('calls onStart when start button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderButton('stopped');

    await user.click(screen.getByLabelText('启动服务'));

    await waitFor(() => {
      expect(props.onStart).toHaveBeenCalledTimes(1);
    });
  });

  it('calls onStop when stop button is clicked', async () => {
    const user = userEvent.setup();
    const { props } = renderButton('running');

    await user.click(screen.getByLabelText('停止服务'));

    await waitFor(() => {
      expect(props.onStop).toHaveBeenCalledTimes(1);
    });
  });
});
