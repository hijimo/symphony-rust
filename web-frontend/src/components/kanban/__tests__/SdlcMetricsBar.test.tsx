import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeProvider } from '@mui/material/styles';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import theme from '../../../theme';
import { server } from '../../../test/mocks/server';
import SdlcMetricsBar from '../SdlcMetricsBar';

const PID = 5;
const full = {
  days: 30,
  intentSurvival: { accepted: 3, rejected: 1, rate: 0.75 },
  designCycle: { count: 2, medianHours: 6, p90Hours: 30.5 },
  rework: { count: 2, flows: 1 },
  firstPassMerge: { closed: 4, firstPass: 3, rate: 0.75, partial: false },
  planConformance: { closed: 2, conforming: 1, rate: 0.5 },
  draftTokens: { input: 1000, cachedInput: 0, output: 234, reasoning: 0, total: 12345 },
};

function renderBar() {
  render(
    <ThemeProvider theme={theme}>
      <SdlcMetricsBar projectId={PID} />
    </ThemeProvider>,
  );
}

describe('SdlcMetricsBar', () => {
  let calls: string[];
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    calls = [];
    server.use(
      http.get(`*/api/projects/${PID}/sdlc/metrics`, ({ request }) => {
        calls.push(new URL(request.url).searchParams.get('days') ?? '');
        return HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data: full });
      }),
    );
  });

  it('shows the six metrics for the last 30 days', async () => {
    renderBar();
    await waitFor(() => expect(screen.getByTestId('metric-intent')).toHaveTextContent('intent 存活率75%（3/4）'));
    expect(screen.getByTestId('metric-cycle')).toHaveTextContent('中位 6h · P90 30.5h');
    expect(screen.getByTestId('metric-rework')).toHaveTextContent('2 次 · 1 个变更');
    expect(screen.getByTestId('metric-firstPass')).toHaveTextContent('75%（3/4）');
    expect(screen.getByTestId('metric-conformance')).toHaveTextContent('50%（1/2）');
    expect(screen.getByTestId('metric-tokens')).toHaveTextContent('12,345');
    expect(screen.getByText('流程度量 · 近 30 天')).toBeInTheDocument();
    expect(calls).toEqual(['30']);
  });

  it('shows dashes when there is no data or the platform failed', async () => {
    server.use(
      http.get(`*/api/projects/${PID}/sdlc/metrics`, () =>
        HttpResponse.json({
          success: true,
          retCode: '0',
          retMsg: 'ok',
          data: {
            ...full,
            intentSurvival: { accepted: 0, rejected: 0, rate: null },
            designCycle: { count: 0, medianHours: null, p90Hours: null },
            firstPassMerge: { rate: null, error: 'GitLab 503' },
          },
        }),
      ),
    );
    renderBar();
    await waitFor(() => expect(screen.getByTestId('metric-intent')).toHaveTextContent('—'));
    expect(screen.getByTestId('metric-cycle')).toHaveTextContent('—');
    expect(screen.getByTestId('metric-firstPass')).toHaveTextContent('—');
  });

  it('collapses and remembers the choice', async () => {
    const user = userEvent.setup();
    renderBar();
    await screen.findByText('intent 存活率');
    await user.click(screen.getByRole('button', { name: '收起度量' }));
    await waitFor(() => expect(screen.queryByText('intent 存活率')).not.toBeVisible());
    expect(localStorage.getItem('sdlcMetricsOpen')).toBe('0');
    expect(screen.getByRole('button', { name: '展开度量' })).toBeInTheDocument();
  });
});
