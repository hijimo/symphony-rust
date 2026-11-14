import { render, screen } from '@testing-library/react';
import { ThemeProvider } from '@mui/material/styles';
import { afterEach, describe, expect, it, vi } from 'vitest';
import theme from '../../../theme';
import { useConcurrencyStore } from '../../../store/concurrencyStore';
import ConcurrencyPanel from '../ConcurrencyPanel';

const original = useConcurrencyStore.getState();

function renderPanel(docSessions: { active: number; max: number; queued: number }) {
  useConcurrencyStore.setState({
    globalMax: 4,
    globalActive: 1,
    utilizationPercent: 25,
    projects: [],
    dataFreshnessSeconds: 0,
    loading: false,
    error: null,
    docSessions,
    fetchStatus: vi.fn(async () => {}),
    connectSSE: vi.fn(),
    disconnectSSE: vi.fn(),
  });
  render(
    <ThemeProvider theme={theme}>
      <ConcurrencyPanel />
    </ThemeProvider>,
  );
}

describe('ConcurrencyPanel document sessions', () => {
  afterEach(() => useConcurrencyStore.setState(original, true));

  it('shows drafting sessions separately from the agent quota', () => {
    renderPanel({ active: 2, max: 2, queued: 3 });
    const box = screen.getByTestId('doc-sessions');
    expect(box).toHaveTextContent('文档起草会话');
    expect(box).toHaveTextContent('2 / 2');
    expect(box).toHaveTextContent('排队 3');
  });

  it('omits the queue count when nothing waits', () => {
    renderPanel({ active: 0, max: 4, queued: 0 });
    expect(screen.getByTestId('doc-sessions')).toHaveTextContent('0 / 4');
    expect(screen.getByTestId('doc-sessions')).not.toHaveTextContent('排队');
  });
});
