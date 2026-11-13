import fs from 'fs';
import os from 'os';
import path from 'path';

/**
 * Playwright may SIGKILL the stack launcher, which leaves its detached
 * children (fake GitLab, web-platform) running. Stop them via the pid file.
 */
export default function teardown() {
  const runDir = process.env.SDLC_E2E_RUN_DIR ?? path.join(os.tmpdir(), 'symphony-sdlc-e2e');
  const pidFile = path.join(runDir, 'pids.json');
  if (!fs.existsSync(pidFile)) return;
  for (const pid of JSON.parse(fs.readFileSync(pidFile, 'utf8')) as number[]) {
    try {
      process.kill(-pid, 'SIGTERM');
    } catch {
      /* already gone */
    }
  }
}
