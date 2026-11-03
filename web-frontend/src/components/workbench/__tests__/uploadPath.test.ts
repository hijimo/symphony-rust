import { describe, expect, it } from 'vitest';
import { normalizeSubdir, uploadTarget } from '../UploadDialog';

describe('upload path helpers', () => {
  it('normalizes sub directories and rejects escapes', () => {
    expect(normalizeSubdir('')).toBe('');
    expect(normalizeSubdir('/assets//评分/')).toBe('assets/评分');
    expect(normalizeSubdir('a\\\\b')).toBe('a/b');
    expect(normalizeSubdir('../x')).toBeNull();
    expect(normalizeSubdir('a/./b')).toBeNull();
    expect(normalizeSubdir('.git')).toBeNull();
  });

  it('joins the writable root, sub directory and file name', () => {
    expect(uploadTarget('docs/changes/8', '', 'a.xlsx')).toBe('docs/changes/8/a.xlsx');
    expect(uploadTarget('primary/docs/changes/8', 'assets', 'a.xlsx')).toBe('primary/docs/changes/8/assets/a.xlsx');
  });
});
