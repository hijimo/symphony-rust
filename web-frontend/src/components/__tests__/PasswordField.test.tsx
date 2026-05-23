import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../theme';
import PasswordField from '../PasswordField';

function renderPasswordField(props = {}) {
  return render(
    <ThemeProvider theme={theme}>
      <PasswordField label="密码" {...props} />
    </ThemeProvider>,
  );
}

describe('PasswordField', () => {
  it('renders a password input', () => {
    renderPasswordField();
    const input = screen.getByLabelText('密码');
    expect(input).toHaveAttribute('type', 'password');
  });

  it('toggles password visibility on button click', async () => {
    const user = userEvent.setup();
    renderPasswordField();

    const input = screen.getByLabelText('密码');
    expect(input).toHaveAttribute('type', 'password');

    const toggleBtn = screen.getByLabelText('显示密码');
    await user.click(toggleBtn);
    expect(input).toHaveAttribute('type', 'text');

    const hideBtn = screen.getByLabelText('隐藏密码');
    await user.click(hideBtn);
    expect(input).toHaveAttribute('type', 'password');
  });

  it('hides toggle button when showToggle is false', () => {
    renderPasswordField({ showToggle: false });
    expect(screen.queryByLabelText('显示密码')).not.toBeInTheDocument();
  });

  it('calls onChange when typing', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderPasswordField({ onChange });

    const input = screen.getByLabelText('密码');
    await user.type(input, 'abc');
    expect(onChange).toHaveBeenCalledTimes(3);
  });
});
