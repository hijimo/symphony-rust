import { describe, expect, it } from 'vitest';
import { diffHunks, revertHunk } from '../hunks';

describe('diffHunks / revertHunk', () => {
  it('finds separate hunks and reverts one independently', () => {
    const before = 'a\nb\nc\nd\ne';
    const after = 'a\nB\nc\nd\nE\nf';
    const hunks = diffHunks(before, after);
    expect(hunks).toHaveLength(2);
    expect(hunks[0]).toMatchObject({ beforeLines: ['b'], afterLines: ['B'] });
    const r = revertHunk(after, hunks[0]);
    expect(r).toBe('a\nb\nc\nd\nE\nf');
    // Reverting the second hunk of the original result restores e and drops f.
    expect(revertHunk(after, hunks[1])).toBe('a\nB\nc\nd\ne');
  });

  it('handles pure insertions and identical text', () => {
    expect(diffHunks('x', 'x')).toEqual([]);
    const h = diffHunks('a\nc', 'a\nb\nc');
    expect(h).toEqual([{ beforeStart: 1, beforeLines: [], afterStart: 1, afterLines: ['b'] }]);
    expect(revertHunk('a\nb\nc', h[0])).toBe('a\nc');
  });
});
