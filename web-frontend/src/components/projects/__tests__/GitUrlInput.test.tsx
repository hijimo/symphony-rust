import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../../theme';
import GitUrlInput, { parseGitUrl } from '../GitUrlInput';

function renderInput(value = '', error?: string) {
  const onChange = vi.fn();
  const result = render(
    <ThemeProvider theme={theme}>
      <GitUrlInput value={value} onChange={onChange} error={error} />
    </ThemeProvider>,
  );
  return { ...result, onChange };
}

import { vi } from 'vitest';

describe('GitUrlInput', () => {
  describe('parseGitUrl', () => {
    it('parses GitHub HTTPS URL correctly', () => {
      const result = parseGitUrl('https://github.com/org/my-repo.git');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBe('github');
      expect(result.namespace).toBe('org');
      expect(result.repoName).toBe('my-repo');
      expect(result.host).toBe('github.com');
    });

    it('parses GitLab HTTPS URL correctly', () => {
      const result = parseGitUrl('https://gitlab.com/group/subgroup/my-project.git');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBe('gitlab');
      expect(result.namespace).toBe('group/subgroup');
      expect(result.repoName).toBe('my-project');
      expect(result.host).toBe('gitlab.com');
    });

    it('parses SSH URL correctly', () => {
      const result = parseGitUrl('git@github.com:org/my-repo.git');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBe('github');
      expect(result.namespace).toBe('org');
      expect(result.repoName).toBe('my-repo');
      expect(result.host).toBe('github.com');
    });

    it('parses GitLab SSH URL correctly', () => {
      const result = parseGitUrl('git@gitlab.com:group/subgroup/my-project.git');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBe('gitlab');
      expect(result.namespace).toBe('group/subgroup');
      expect(result.repoName).toBe('my-project');
    });

    it('returns invalid for empty string', () => {
      const result = parseGitUrl('');
      expect(result.isValid).toBe(false);
      expect(result.platform).toBeNull();
    });

    it('returns invalid for malformed URL', () => {
      const result = parseGitUrl('not-a-valid-url');
      expect(result.isValid).toBe(false);
      expect(result.platform).toBeNull();
    });

    it('handles HTTPS URL without .git suffix', () => {
      const result = parseGitUrl('https://github.com/org/my-repo');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBe('github');
      expect(result.repoName).toBe('my-repo');
    });

    it('returns null platform for unknown host', () => {
      const result = parseGitUrl('https://bitbucket.org/team/repo.git');
      expect(result.isValid).toBe(true);
      expect(result.platform).toBeNull();
    });
  });

  describe('component rendering', () => {
    it('renders text field with label', () => {
      renderInput();
      expect(screen.getByLabelText(/Git URL/)).toBeInTheDocument();
    });

    it('shows error message when error prop is provided', () => {
      renderInput('', '无效的 Git URL 格式');
      expect(screen.getByText('无效的 Git URL 格式')).toBeInTheDocument();
    });

    it('shows helper text when no error', () => {
      renderInput();
      expect(screen.getByText('支持 HTTPS 和 SSH 格式')).toBeInTheDocument();
    });

    it('displays parsed platform, namespace, and repo for valid URL', () => {
      renderInput('https://github.com/org/my-repo.git');
      expect(screen.getByText('解析结果')).toBeInTheDocument();
      expect(screen.getByText('GitHub')).toBeInTheDocument();
      expect(screen.getByText('org')).toBeInTheDocument();
      expect(screen.getByText('my-repo')).toBeInTheDocument();
    });

    it('does not show parsed result for invalid URL', () => {
      renderInput('invalid-url');
      expect(screen.queryByText('解析结果')).not.toBeInTheDocument();
    });

    it('calls onChange when user types', async () => {
      const user = userEvent.setup();
      const { onChange } = renderInput();

      const input = screen.getByLabelText(/Git URL/);
      await user.type(input, 'a');

      expect(onChange).toHaveBeenCalledWith('a');
    });
  });
});
