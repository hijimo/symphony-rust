import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import KanbanBoard from '../KanbanBoard';
import type { KanbanData, KanbanMergeRequest } from '../../../types/kanban';

const author = {
  username: 'alice',
  display_name: 'Alice',
  avatar_url: null,
};

function createMergeRequest(
  iid: number,
  title: string,
  state: KanbanMergeRequest['state'],
  updatedAt: string,
  createdAt: string,
): KanbanMergeRequest {
  return {
    iid,
    title,
    state,
    repository: 'hijimo/symphony-rust',
    author,
    source_branch: `feature/${iid}`,
    target_branch: 'main',
    ci_status: 'success',
    review_status: 'pending',
    related_issue_iids: [],
    created_at: createdAt,
    updated_at: updatedAt,
    web_url: `https://example.test/pulls/${iid}`,
  };
}

function createKanbanData(mergeRequests: KanbanMergeRequest[]): KanbanData {
  return {
    todo: {
      issues: [],
      total_count: 0,
      has_more: false,
    },
    in_progress: {
      issues: [],
      total_count: 0,
    },
    pr: {
      merge_requests: mergeRequests,
      total_count: mergeRequests.length,
    },
    cached: false,
    cached_at: null,
    platform: 'github',
  };
}

function renderBoard(data: KanbanData) {
  return render(
    <ThemeProvider theme={theme}>
      <KanbanBoard data={data} />
    </ThemeProvider>,
  );
}

describe('KanbanBoard', () => {
  it('shows only pending PRs and orders them by updated time stably', () => {
    const data = createKanbanData([
      createMergeRequest(
        3,
        'Closed terminal PR',
        'closed',
        '2026-05-24T11:00:00Z',
        '2026-05-24T08:00:00Z',
      ),
      createMergeRequest(
        4,
        'Older pending PR',
        'opened',
        '2026-05-24T10:00:00Z',
        '2026-05-24T09:00:00Z',
      ),
      createMergeRequest(
        2,
        'Merged terminal PR',
        'merged',
        '2026-05-24T12:00:00Z',
        '2026-05-24T07:00:00Z',
      ),
      createMergeRequest(
        1,
        'Newer pending PR',
        'opened',
        '2026-05-24T10:00:00Z',
        '2026-05-24T09:30:00Z',
      ),
    ]);

    renderBoard(data);

    const prColumn = screen.getByText('PR').closest('div')?.parentElement;
    expect(prColumn).toBeInTheDocument();

    expect(screen.queryByText('Closed terminal PR')).not.toBeInTheDocument();
    expect(screen.queryByText('Merged terminal PR')).not.toBeInTheDocument();

    const links = within(prColumn as HTMLElement).getAllByRole('link');
    expect(links.map((link) => link.textContent)).toEqual([
      '!1Newer pending PR',
      '!4Older pending PR',
    ]);
    expect(within(prColumn as HTMLElement).getByText('2')).toBeInTheDocument();
  });

  it('shows pending PR empty state after terminal PRs are filtered out', () => {
    const data = createKanbanData([
      createMergeRequest(
        2,
        'Merged terminal PR',
        'merged',
        '2026-05-24T12:00:00Z',
        '2026-05-24T07:00:00Z',
      ),
      createMergeRequest(
        3,
        'Closed terminal PR',
        'closed',
        '2026-05-24T11:00:00Z',
        '2026-05-24T08:00:00Z',
      ),
    ]);

    renderBoard(data);

    expect(screen.queryByText('Merged terminal PR')).not.toBeInTheDocument();
    expect(screen.queryByText('Closed terminal PR')).not.toBeInTheDocument();
    expect(screen.getByText('暂无待处理 PR')).toBeInTheDocument();
  });
});
