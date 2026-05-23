import { create } from 'zustand';
import type { UserInfo } from '../types';

interface AuthState {
  token: string | null;
  expiresAt: string | null;
  user: UserInfo | null;
  isAuthenticated: boolean;
  login: (token: string, user: UserInfo, expiresAt: string) => void;
  logout: () => void;
  setUser: (user: UserInfo) => void;
}

function loadUser(): UserInfo | null {
  const raw = localStorage.getItem('user');
  if (!raw) return null;
  try {
    return JSON.parse(raw) as UserInfo;
  } catch {
    localStorage.removeItem('user');
    localStorage.removeItem('token');
    localStorage.removeItem('expiresAt');
    return null;
  }
}

export const useAuthStore = create<AuthState>((set) => ({
  token: localStorage.getItem('token'),
  expiresAt: localStorage.getItem('expiresAt'),
  user: loadUser(),
  isAuthenticated: !!localStorage.getItem('token'),

  login: (token, user, expiresAt) => {
    localStorage.setItem('token', token);
    localStorage.setItem('user', JSON.stringify(user));
    localStorage.setItem('expiresAt', expiresAt);
    set({ token, user, expiresAt, isAuthenticated: true });
  },

  logout: () => {
    localStorage.removeItem('token');
    localStorage.removeItem('user');
    localStorage.removeItem('expiresAt');
    set({ token: null, user: null, expiresAt: null, isAuthenticated: false });
  },

  setUser: (user) => {
    localStorage.setItem('user', JSON.stringify(user));
    set({ user });
  },
}));
