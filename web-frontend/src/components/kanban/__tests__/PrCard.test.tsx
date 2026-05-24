import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import PrCard from '../PrCard';
import type { KanbanMergeRequest } from '../../../types/kanban';

const pendingMr: KanbanMergeRequest = {
  iid: 42,
  title: 'Add kanban pending PR column',
  state: 'opened',
  repository: 'group/symphony',
  author: {
    username: 'alice',
    display_name: 'Alice',
    avatar_url: null,
  },
  source_branch: 'feature/pending-prs',
  target_branch: 'main',
  ci_status: 'success',
  review_status: 'pending',
  related_issue_iids: [3],
  created_at: '2026-05-20T10:00:00Z',
  updated_at: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
  web_url: 'https://gitlab.example.com/group/symphony/-/merge_requests/42',
};

function renderCard(mr: KanbanMergeRequest = pendingMr) {
  return render(
    <ThemeProvider theme={theme}>
      <PrCard mr={mr} />
    </ThemeProvider>,
  );
}

describe('PrCard', () => {
  it('shows title, source project, author, updated time, and current state', () => {
    renderCard();

    expect(screen.getByRole('link', { name: /Add kanban pending PR column/ })).toHaveAttribute(
      'href',
      pendingMr.web_url,
    );
    expect(screen.getByText('group/symphony')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('开启')).toBeInTheDocument();
    expect(screen.getByText(/小时前|分钟前|刚刚/)).toBeInTheDocument();
  });

  it('shows pending state label', () => {
    renderCard({ ...pendingMr, state: 'pending' });

    expect(screen.getByText('待处理')).toBeInTheDocument();
  });
});
