import client from './client';
import type {
  ResponseData,
  UserProfile,
  UserConfig,
  UpdateProfileParams,
  UpdateConfigParams,
  ChangePasswordParams,
} from '../types';

export async function getProfile(): Promise<UserProfile> {
  const res = await client.get<ResponseData<UserProfile>>('/user/profile');
  return res.data.data;
}

export async function updateProfile(data: UpdateProfileParams): Promise<void> {
  await client.put<ResponseData>('/user/profile', data);
}

export async function getConfig(): Promise<UserConfig> {
  const res = await client.get<ResponseData<UserConfig>>('/user/config');
  return res.data.data;
}

export async function updateConfig(data: UpdateConfigParams): Promise<void> {
  await client.put<ResponseData>('/user/config', data);
}

export async function changePassword(data: ChangePasswordParams): Promise<void> {
  await client.put<ResponseData>('/auth/password', data);
}
