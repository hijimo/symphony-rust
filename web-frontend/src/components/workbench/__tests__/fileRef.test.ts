import { describe, expect, it } from 'vitest';
import { parseFileRef } from '../AiPanel';

describe('parseFileRef', () => {
  it('parses paths with and without line numbers', () => {
    expect(parseFileRef('`src/main.rs`')).toEqual({ path: 'src/main.rs', line: null });
    expect(parseFileRef('`src/main.rs:12`')).toEqual({ path: 'src/main.rs', line: 12 });
    expect(parseFileRef('`not a path`')).toBeNull();
    expect(parseFileRef('src/main.rs')).toBeNull();
  });
});
