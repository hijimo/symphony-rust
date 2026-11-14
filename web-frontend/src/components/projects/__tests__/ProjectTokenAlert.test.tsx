import { render, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { beforeEach, describe, expect, it } from 'vitest';
import { server } from '../../../test/mocks/server';
import ProjectTokenAlert from '../ProjectTokenAlert';

function status(data: unknown) {
  return http.get('*/api/projects/7/token-status', () =>
    HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data }),
  );
}

describe('ProjectTokenAlert', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
  });

  it('shows the reason when the project token is unusable', async () => {
    server.use(status({ ok: false, reason: '项目 Token 不可用：平台 Token 已失效或被撤销', checkedAt: 'x' }));
    render(<ProjectTokenAlert projectId={7} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('平台 Token 已失效或被撤销');
  });

  it('renders nothing when the token works or the check fails', async () => {
    let calls = 0;
    server.use(
      http.get('*/api/projects/7/token-status', () => {
        calls += 1;
        return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: { ok: true, reason: null, checkedAt: 'x' } });
      }),
    );
    const { container } = render(<ProjectTokenAlert projectId={7} />);
    await waitFor(() => expect(calls).toBe(1));
    expect(container).toBeEmptyDOMElement();

    server.use(http.get('*/api/projects/8/token-status', () => HttpResponse.json({}, { status: 500 })));
    const other = render(<ProjectTokenAlert projectId={8} />);
    await new Promise((r) => setTimeout(r, 50));
    expect(other.container).toBeEmptyDOMElement();
  });
});
