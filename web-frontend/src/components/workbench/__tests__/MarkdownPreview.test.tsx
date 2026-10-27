import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import MarkdownPreview, { stripFrontmatter } from '../MarkdownPreview';
import { INTENT_TEXT } from '../../../test/workbenchFixtures';

describe('stripFrontmatter', () => {
  it('removes a leading frontmatter block', () => {
    expect(stripFrontmatter('---\na: 1\n---\n# T\n')).toBe('# T\n');
  });
  it('keeps text without frontmatter or with unterminated frontmatter', () => {
    expect(stripFrontmatter('# T\n---\n')).toBe('# T\n---\n');
    expect(stripFrontmatter('---\na: 1\n# T\n')).toBe('---\na: 1\n# T\n');
  });
});

describe('MarkdownPreview', () => {
  it('renders GFM tables', () => {
    render(<MarkdownPreview content={'| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n'} />);
    const table = screen.getByRole('table');
    expect(within(table).getAllByRole('columnheader').map((c) => c.textContent)).toEqual(['a', 'b']);
    expect(within(table).getAllByRole('cell').map((c) => c.textContent)).toEqual(['1', '2', '3', '4']);
  });

  it('renders other GFM syntax (strikethrough, task list)', () => {
    render(<MarkdownPreview content={'~~old~~\n\n- [x] done\n- [ ] todo\n'} />);
    const preview = screen.getByTestId('markdown-preview');
    expect(preview.querySelector('del')?.textContent).toBe('old');
    expect(screen.getAllByRole('checkbox')).toHaveLength(2);
  });

  it('does not display the system frontmatter', () => {
    render(<MarkdownPreview content={INTENT_TEXT} />);
    const preview = screen.getByTestId('markdown-preview');
    expect(preview.textContent).not.toContain('symphony');
    expect(preview.textContent).not.toContain('doc: intent');
    expect(preview.querySelector('hr')).toBeNull();
    expect(screen.getByRole('heading', { level: 1, name: 'Intent: X' })).toBeInTheDocument();
    expect(screen.getByRole('table')).toBeInTheDocument();
  });

  it('maps h2 clicks to source lines including the stripped frontmatter offset', async () => {
    const user = userEvent.setup();
    const onLineClick = vi.fn();
    render(<MarkdownPreview content={INTENT_TEXT} onLineClick={onLineClick} />);
    const h2 = screen.getByRole('heading', { level: 2, name: 'Problem' });
    // "## Problem" is line 7 of INTENT_TEXT (1-based)
    expect(INTENT_TEXT.split('\n')[6]).toBe('## Problem');
    expect(h2).toHaveAttribute('data-line', '7');
    await user.click(h2);
    expect(onLineClick).toHaveBeenCalledWith(7);
  });

  it('renders mermaid fences into a mermaid container', () => {
    render(<MarkdownPreview content={'```mermaid\ngraph TD; A-->B;\n```\n'} />);
    expect(screen.getByTestId('mermaid')).toBeInTheDocument();
  });
});
