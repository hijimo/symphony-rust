import client from './client';
import type {
  ResponseData,
  PaginationData,
  UserProfile,
  CreateUserParams,
  ResetPasswordParams,
  GetUsersParams,
} from '../types';

export async function getUsers(params: GetUsersParams): Promise<PaginationData<UserProfile>> {
  const res = await client.get<ResponseData<PaginationData<UserProfile>>>('/admin/users', {
    params,
  });
  return res.data.data;
}

export async function createUser(data: CreateUserParams): Promise<void> {
  await client.post<ResponseData>('/admin/users', data);
}

export async function deleteUser(id: number): Promise<void> {
  await client.delete<ResponseData>(`/admin/users/${id}`);
}

export async function resetPassword(id: number, data: ResetPasswordParams): Promise<void> {
  await client.put<ResponseData>(`/admin/users/${id}/reset-password`, data);
}
