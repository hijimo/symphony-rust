// Line-level diff + hunk revert used by the "按块回退" flow (workbench §3.3).

export interface Hunk {
  /** 0-based line index in `before` where the hunk starts. */
  beforeStart: number;
  beforeLines: string[];
  afterStart: number;
  afterLines: string[];
}

/** LCS-based line diff, returning change hunks. */
export function diffHunks(before: string, after: string): Hunk[] {
  const a = before.split('\n');
  const b = after.split('\n');
  const n = a.length;
  const m = b.length;
  // LCS table (documents are small; O(n*m) is fine up to a few thousand lines).
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const hunks: Hunk[] = [];
  let i = 0;
  let j = 0;
  let cur: Hunk | null = null;
  const flush = () => {
    if (cur) hunks.push(cur);
    cur = null;
  };
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      flush();
      i++;
      j++;
    } else if (j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j])) {
      cur = cur ?? { beforeStart: i, beforeLines: [], afterStart: j, afterLines: [] };
      cur.afterLines.push(b[j++]);
    } else {
      cur = cur ?? { beforeStart: i, beforeLines: [], afterStart: j, afterLines: [] };
      cur.beforeLines.push(a[i++]);
    }
  }
  flush();
  return hunks;
}

/** Revert one hunk of `after` back to `before`. */
export function revertHunk(after: string, hunk: Hunk): string {
  const lines = after.split('\n');
  lines.splice(hunk.afterStart, hunk.afterLines.length, ...hunk.beforeLines);
  return lines.join('\n');
}
