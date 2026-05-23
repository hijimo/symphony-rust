import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import LabelSelector from '../LabelSelector';

function renderLabelSelector(onChange = vi.fn()) {
  render(
    <ThemeProvider theme={theme}>
      <LabelSelector value={[]} onChange={onChange} />
    </ThemeProvider>,
  );
  return onChange;
}

describe('LabelSelector', () => {
  it('commits a typed label when the input loses focus', async () => {
    const user = userEvent.setup();
    const onChange = renderLabelSelector();

    await user.type(screen.getByLabelText('标签'), 'Todo');
    await user.tab();

    await waitFor(() => {
      expect(onChange).toHaveBeenLastCalledWith(['Todo']);
    });
  });

  it('parses multiple comma-separated labels without changing label names', async () => {
    const user = userEvent.setup();
    const onChange = renderLabelSelector();

    await user.type(screen.getByLabelText('标签'), 'Todo,In Progress');
    await user.keyboard('{Enter}');

    await waitFor(() => {
      expect(onChange).toHaveBeenLastCalledWith(['Todo', 'In Progress']);
    });
  });
});
