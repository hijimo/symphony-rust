import client from './client';
import type { LoginRequest, LoginResponse } from './types';
import type { ResponseData } from '../types';

export async function login(data: LoginRequest): Promise<LoginResponse> {
  const res = await client.post<ResponseData<LoginResponse>>('/auth/login', data);
  return res.data.data;
}
