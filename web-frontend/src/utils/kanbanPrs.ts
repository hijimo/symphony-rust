import type { KanbanMergeRequest } from '../types/kanban';

function toTimestamp(value: string): number {
  const timestamp = new Date(value).getTime();
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

export function isPendingMergeRequest(mr: KanbanMergeRequest): boolean {
  return mr.state.trim().toLowerCase() === 'opened';
}

export function getPendingMergeRequests(
  mergeRequests: KanbanMergeRequest[],
): KanbanMergeRequest[] {
  return mergeRequests
    .filter(isPendingMergeRequest)
    .sort((a, b) => {
      const updatedDiff = toTimestamp(b.updated_at) - toTimestamp(a.updated_at);
      if (updatedDiff !== 0) return updatedDiff;

      const createdDiff = toTimestamp(b.created_at) - toTimestamp(a.created_at);
      if (createdDiff !== 0) return createdDiff;

      return b.iid - a.iid;
    });
}
