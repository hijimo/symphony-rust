import { describe, it, expect, beforeEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../test/mocks/server';
import client from '../client';

describe('API Client', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(window, 'location', {
      writable: true,
      value: { href: 'http://localhost/', pathname: '/' },
    });
  });

  it('attaches Authorization header when token exists', async () => {
    localStorage.setItem('token', 'my-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');

    let capturedAuth = '';
    server.use(
      http.get('*/api/test', ({ request }) => {
        capturedAuth = request.headers.get('Authorization') || '';
        return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
      }),
    );

    await client.get('/test');
    expect(capturedAuth).toBe('Bearer my-token');
  });

  it('does not attach Authorization header when no token', async () => {
    let capturedAuth: string | null = 'initial';
    server.use(
      http.get('*/api/test', ({ request }) => {
        capturedAuth = request.headers.get('Authorization');
        return HttpResponse.json({ success: true, retCode: 'SUCCESS', retMsg: 'ok', data: null });
      }),
    );

    await client.get('/test');
    expect(capturedAuth).toBeNull();
  });

  it('clears token and redirects on AUTH_001 response (success=false)', async () => {
    localStorage.setItem('token', 'expired-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    server.use(
      http.get('*/api/test', () => {
        return HttpResponse.json({
          success: false,
          retCode: 'AUTH_001',
          retMsg: '登录已过期',
          data: null,
        });
      }),
    );

    await expect(client.get('/test')).rejects.toThrow('登录已过期');
    expect(localStorage.getItem('token')).toBeNull();
    expect(window.location.href).toBe('/login');
  });

  it('clears token and redirects on AUTH_001 in error response', async () => {
    localStorage.setItem('token', 'expired-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    server.use(
      http.get('*/api/test', () => {
        return HttpResponse.json(
          { success: false, retCode: 'AUTH_001', retMsg: '未授权', data: null },
          { status: 401 },
        );
      }),
    );

    await expect(client.get('/test')).rejects.toThrow('未授权');
    expect(localStorage.getItem('token')).toBeNull();
    expect(window.location.href).toBe('/login');
  });

  it('handles network errors', async () => {
    server.use(
      http.get('*/api/test', () => {
        return HttpResponse.error();
      }),
    );

    await expect(client.get('/test')).rejects.toThrow('网络连接失败，请检查网络后重试');
  });

  it('handles rate limiting (429)', async () => {
    server.use(
      http.get('*/api/test', () => {
        return new HttpResponse(null, { status: 429 });
      }),
    );

    await expect(client.get('/test')).rejects.toThrow('登录尝试过于频繁，请稍后再试');
  });

  it('returns generic error for non-AUTH_001 failures', async () => {
    server.use(
      http.get('*/api/test', () => {
        return HttpResponse.json(
          { success: false, retCode: 'ERR_500', retMsg: '服务器内部错误', data: null },
          { status: 500 },
        );
      }),
    );

    await expect(client.get('/test')).rejects.toThrow('服务器内部错误');
  });

  it('rejects and redirects when token is about to expire', async () => {
    localStorage.setItem('token', 'expiring-token');
    localStorage.setItem('expiresAt', new Date(Date.now() + 1000).toISOString());

    await expect(client.get('/test')).rejects.toThrow('登录已过期，请重新登录');
    expect(localStorage.getItem('token')).toBeNull();
    expect(localStorage.getItem('expiresAt')).toBeNull();
    expect(window.location.href).toBe('/login');
  });
});
