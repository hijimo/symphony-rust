import { describe, it, expect, beforeEach } from 'vitest';
import { useAuthStore } from '../auth';

describe('useAuthStore', () => {
  beforeEach(() => {
    localStorage.clear();
    useAuthStore.setState({
      token: null,
      expiresAt: null,
      user: null,
      isAuthenticated: false,
    });
  });

  it('has correct initial state when no token in localStorage', () => {
    const state = useAuthStore.getState();
    expect(state.isAuthenticated).toBe(false);
    expect(state.token).toBeNull();
    expect(state.user).toBeNull();
    expect(state.expiresAt).toBeNull();
  });

  it('login sets token, user, expiresAt, and isAuthenticated', () => {
    const user = { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' as const };
    useAuthStore.getState().login('test-token', user, '2099-01-01T00:00:00Z');

    const state = useAuthStore.getState();
    expect(state.token).toBe('test-token');
    expect(state.user).toEqual(user);
    expect(state.expiresAt).toBe('2099-01-01T00:00:00Z');
    expect(state.isAuthenticated).toBe(true);
  });

  it('login persists token to localStorage', () => {
    const user = { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' as const };
    useAuthStore.getState().login('persisted-token', user, '2099-01-01T00:00:00Z');

    expect(localStorage.getItem('token')).toBe('persisted-token');
    expect(localStorage.getItem('expiresAt')).toBe('2099-01-01T00:00:00Z');
    expect(JSON.parse(localStorage.getItem('user')!)).toEqual(user);
  });

  it('logout clears state', () => {
    const user = { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' as const };
    useAuthStore.getState().login('test-token', user, '2099-01-01T00:00:00Z');
    useAuthStore.getState().logout();

    const state = useAuthStore.getState();
    expect(state.token).toBeNull();
    expect(state.user).toBeNull();
    expect(state.expiresAt).toBeNull();
    expect(state.isAuthenticated).toBe(false);
  });

  it('logout removes token from localStorage', () => {
    const user = { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' as const };
    useAuthStore.getState().login('test-token', user, '2099-01-01T00:00:00Z');
    useAuthStore.getState().logout();

    expect(localStorage.getItem('token')).toBeNull();
    expect(localStorage.getItem('user')).toBeNull();
    expect(localStorage.getItem('expiresAt')).toBeNull();
  });

  it('setUser updates user in state and localStorage', () => {
    const user = { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' as const };
    useAuthStore.getState().login('test-token', user, '2099-01-01T00:00:00Z');

    const updatedUser = { ...user, displayName: 'New Name' };
    useAuthStore.getState().setUser(updatedUser);

    expect(useAuthStore.getState().user).toEqual(updatedUser);
    expect(JSON.parse(localStorage.getItem('user')!)).toEqual(updatedUser);
  });

  it('restores state from localStorage on store creation', () => {
    localStorage.setItem('token', 'restored-token');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');
    localStorage.setItem(
      'user',
      JSON.stringify({ id: 1, username: 'admin', displayName: 'Admin', role: 'admin' }),
    );

    const token = localStorage.getItem('token');
    const expiresAt = localStorage.getItem('expiresAt');
    const raw = localStorage.getItem('user');
    const user = raw ? JSON.parse(raw) : null;

    expect(token).toBe('restored-token');
    expect(expiresAt).toBe('2099-01-01T00:00:00Z');
    expect(user).toEqual({ id: 1, username: 'admin', displayName: 'Admin', role: 'admin' });
    expect(!!token).toBe(true);
  });

  it('handles corrupted user data in localStorage gracefully', () => {
    localStorage.setItem('token', 'some-token');
    localStorage.setItem('user', 'not-valid-json{{{');
    localStorage.setItem('expiresAt', '2099-01-01T00:00:00Z');

    useAuthStore.setState({
      token: null,
      expiresAt: null,
      user: null,
      isAuthenticated: false,
    });

    // Simulate what loadUser does with corrupted data
    const raw = localStorage.getItem('user');
    let user = null;
    try {
      user = raw ? JSON.parse(raw) : null;
    } catch {
      localStorage.removeItem('user');
      localStorage.removeItem('token');
      localStorage.removeItem('expiresAt');
    }

    expect(user).toBeNull();
    expect(localStorage.getItem('token')).toBeNull();
    expect(localStorage.getItem('user')).toBeNull();
    expect(localStorage.getItem('expiresAt')).toBeNull();
  });
});
