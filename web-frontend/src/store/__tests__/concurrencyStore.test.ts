import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { server } from '../../test/mocks/server';
import { useConcurrencyStore } from '../concurrencyStore';

const ok = (data: unknown) => HttpResponse.json({ success: true, retCode: '0', retMsg: 'ok', data });
const status = (extra: object = {}) => ({
  global_max: 4,
  global_active: 1,
  utilization_percent: 25,
  projects: [],
  data_freshness_seconds: 2,
  ...extra,
});

describe('concurrencyStore', () => {
  beforeEach(() => {
    localStorage.setItem('token', 'mock-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    useConcurrencyStore.getState().reset();
  });
  afterEach(() => vi.useRealTimers());

  it('fetchStatus maps document sessions', async () => {
    server.use(http.get('*/api/admin/concurrency', () => ok(status({ doc_sessions: { active: 2, max: 3, queued: 1 } }))));
    await useConcurrencyStore.getState().fetchStatus();
    const s = useConcurrencyStore.getState();
    expect(s.globalMax).toBe(4);
    expect(s.globalActive).toBe(1);
    expect(s.dataFreshnessSeconds).toBe(2);
    expect(s.docSessions).toEqual({ active: 2, max: 3, queued: 1 });
    expect(s.loading).toBe(false);
  });

  it('fetchStatus defaults document sessions for older backends and reports errors', async () => {
    server.use(http.get('*/api/admin/concurrency', () => ok(status())));
    await useConcurrencyStore.getState().fetchStatus();
    expect(useConcurrencyStore.getState().docSessions).toEqual({ active: 0, max: 0, queued: 0 });

    server.use(http.get('*/api/admin/concurrency', () => HttpResponse.json({}, { status: 500 })));
    await useConcurrencyStore.getState().fetchStatus();
    expect(useConcurrencyStore.getState().error).toBe('Failed to fetch concurrency status');
  });

  it('updateConfig stores the new limit and rethrows failures', async () => {
    server.use(http.put('*/api/admin/concurrency/config', () => ok({ global_max: 8, previous_value: 4 })));
    await useConcurrencyStore.getState().updateConfig({ globalMax: 8, expectedPrevious: 4 });
    expect(useConcurrencyStore.getState().globalMax).toBe(8);

    server.use(http.put('*/api/admin/concurrency/config', () => HttpResponse.json({}, { status: 409 })));
    await expect(useConcurrencyStore.getState().updateConfig({ globalMax: 9 })).rejects.toBeTruthy();
    expect(useConcurrencyStore.getState().globalMax).toBe(8);
  });

  it('applies SSE snapshots and agent events', () => {
    const st = useConcurrencyStore.getState();
    st.handleSSEEvent({ type: 'snapshot', global_active: 2, global_max: 4, projects: [{ project_id: 1 }] });
    expect(useConcurrencyStore.getState().utilizationPercent).toBe(50);
    expect(useConcurrencyStore.getState().projects).toHaveLength(1);
    st.handleSSEEvent({ type: 'agent_started', global_active: 3 });
    expect(useConcurrencyStore.getState().globalActive).toBe(3);
    st.handleSSEEvent({ type: 'other', global_active: 0 });
    expect(useConcurrencyStore.getState().globalActive).toBe(3);
    st.handleSSEEvent({ type: 'agent_completed', global_active: 0, global_max: 0 });
    expect(useConcurrencyStore.getState().utilizationPercent).toBe(0);
  });

  it('connects, disconnects and reconnects the event stream', async () => {
    vi.useFakeTimers();
    const instances: { close: ReturnType<typeof vi.fn>; onopen?: () => void; onerror?: () => void; onmessage?: (e: { data: string }) => void }[] = [];
    vi.stubGlobal(
      'EventSource',
      vi.fn(function (this: (typeof instances)[number]) {
        this.close = vi.fn();
        instances.push(this);
      }),
    );
    server.use(http.post('*/api/admin/concurrency/events/ticket', () => ok({ ticket: 't', expires_at: '' })));
    const st = useConcurrencyStore.getState();
    await st.connectSSE();
    expect(instances).toHaveLength(1);
    await st.connectSSE(); // already connected
    expect(instances).toHaveLength(1);

    instances[0].onopen?.();
    expect(useConcurrencyStore.getState().sseConnected).toBe(true);
    instances[0].onmessage?.({ data: JSON.stringify({ type: 'agent_started', global_active: 4, global_max: 4 }) });
    instances[0].onmessage?.({ data: 'not json' });
    expect(useConcurrencyStore.getState().globalActive).toBe(4);

    instances[0].onerror?.();
    expect(instances[0].close).toHaveBeenCalled();
    expect(useConcurrencyStore.getState().sseConnected).toBe(false);
    await vi.advanceTimersByTimeAsync(3000);
    expect(instances).toHaveLength(2);

    useConcurrencyStore.getState().disconnectSSE();
    expect(instances[1].close).toHaveBeenCalled();
    expect(useConcurrencyStore.getState().eventSource).toBeNull();

    server.use(http.post('*/api/admin/concurrency/events/ticket', () => HttpResponse.json({}, { status: 500 })));
    await useConcurrencyStore.getState().connectSSE();
    expect(useConcurrencyStore.getState().sseConnected).toBe(false);
    vi.unstubAllGlobals();
  });
});
