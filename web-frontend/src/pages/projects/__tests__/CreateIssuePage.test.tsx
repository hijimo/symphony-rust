import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import CreateIssuePage from '../CreateIssuePage';
import { createIssue } from '../../../api/issues';

const mockNavigate = vi.fn();

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

vi.mock('../../../api/issues', () => ({
  createIssue: vi.fn(),
}));

function renderCreateIssuePage() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={['/projects/42/issues/new']}>
        <Routes>
          <Route path="/projects/:id/issues/new" element={<CreateIssuePage />} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('CreateIssuePage', () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    vi.mocked(createIssue).mockReset();
    vi.mocked(createIssue).mockResolvedValue({
      iid: 7,
      title: 'Issue with label',
      description: null,
      state: 'opened',
      labels: ['Todo'],
      author: { username: 'octo', displayName: null, avatarUrl: null },
      assignees: [],
      milestone: null,
      createdAt: '2026-05-23T00:00:00Z',
      updatedAt: '2026-05-23T00:00:00Z',
      closedAt: null,
      webUrl: 'https://example.com/issues/7',
      commentCount: 0,
      relatedMrs: [],
    });
  });

  it('submits the currently typed label when the user clicks create without pressing Enter', async () => {
    const user = userEvent.setup();
    renderCreateIssuePage();

    await user.type(screen.getByRole('textbox', { name: /Issue 标题/ }), 'Issue with label');
    await user.type(screen.getByRole('combobox', { name: '标签' }), 'Todo');
    await user.click(screen.getByRole('button', { name: '创建 Issue' }));

    await waitFor(() => {
      expect(createIssue).toHaveBeenCalledWith(42, {
        title: 'Issue with label',
        description: undefined,
        labels: ['Todo'],
        assignee: undefined,
      });
    });
  });
});
