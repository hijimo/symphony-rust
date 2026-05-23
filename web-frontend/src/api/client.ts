import axios from 'axios';
import type { ResponseData } from '../types';

const client = axios.create({
  baseURL: '/api',
  timeout: 15000,
  headers: {
    'Content-Type': 'application/json',
  },
});

client.interceptors.request.use((config) => {
  const token = localStorage.getItem('token');
  if (token) {
    const expiresAt = localStorage.getItem('expiresAt');
    if (expiresAt) {
      const remaining = new Date(expiresAt).getTime() - Date.now();
      if (remaining < 3600000) {
        localStorage.removeItem('token');
        localStorage.removeItem('user');
        localStorage.removeItem('expiresAt');
        window.location.href = '/login';
        return Promise.reject(new Error('登录已过期，请重新登录'));
      }
    }
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

client.interceptors.response.use(
  (response) => {
    const body = response.data as ResponseData<unknown>;
    if (!body.success) {
      if (body.retCode === 'AUTH_001') {
        localStorage.removeItem('token');
        localStorage.removeItem('user');
        localStorage.removeItem('expiresAt');
        window.location.href = '/login';
        return Promise.reject(new Error(body.retMsg));
      }
      return Promise.reject(new Error(body.retMsg));
    }
    return response;
  },
  (error) => {
    if (!axios.isAxiosError(error)) {
      return Promise.reject(error);
    }
    if (error.response?.status === 429) {
      return Promise.reject(new Error('登录尝试过于频繁，请稍后再试'));
    }
    if (!error.response) {
      return Promise.reject(new Error('网络连接失败，请检查网络后重试'));
    }
    const body = error.response.data as ResponseData<unknown> | undefined;
    if (body?.retCode === 'AUTH_001') {
      localStorage.removeItem('token');
      localStorage.removeItem('user');
      localStorage.removeItem('expiresAt');
      window.location.href = '/login';
    }
    return Promise.reject(new Error(body?.retMsg ?? '服务器异常，请稍后再试'));
  },
);

export default client;
