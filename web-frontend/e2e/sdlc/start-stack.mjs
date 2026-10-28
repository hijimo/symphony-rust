#!/usr/bin/env node
/**
 * Starts the isolated SDLC E2E backend stack in the foreground:
 *   1. fake GitLab (REST v4 + git smart HTTP) on SDLC_E2E_GITLAB_PORT,
 *   2. the real web-platform (cargo build output) on SDLC_E2E_BACKEND_PORT with
 *      its own DATABASE_URL / workspace root and SYMPHONY_DOC_AGENT_COMMAND
 *      pointing at the fake codex app-server.
 * Everything lives under SDLC_E2E_RUN_DIR, which is recreated on each start.
 * Used by playwright.sdlc.config.ts `webServer`; can also be run by hand.
 */
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../../..');
const backendPort = Number(process.env.SDLC_E2E_BACKEND_PORT ?? 3917);
const gitlabPort = Number(process.env.SDLC_E2E_GITLAB_PORT ?? 18929);
const webPort = Number(process.env.SDLC_E2E_WEB_PORT ?? 5197);
const runDir = process.env.SDLC_E2E_RUN_DIR ?? path.join(os.tmpdir(), 'symphony-sdlc-e2e');

// Fresh run directory (only ever our own temp dir).
if (!runDir.includes('symphony-sdlc-e2e')) throw new Error(`refusing to reset unexpected run dir ${runDir}`);
// Kill stragglers of a previous run of THIS stack (pids recorded below).
const pidFile = path.join(runDir, 'pids.json');
if (fs.existsSync(pidFile)) {
  for (const pid of JSON.parse(fs.readFileSync(pidFile, 'utf8'))) {
    try {
      process.kill(-pid, 'SIGTERM');
    } catch {
      /* already gone */
    }
  }
}
fs.rmSync(runDir, { recursive: true, force: true });
fs.mkdirSync(path.join(runDir, 'git'), { recursive: true });
fs.mkdirSync(path.join(runDir, 'ws'), { recursive: true });

const children = [];
const stop = () => {
  for (const c of children) {
    try {
      process.kill(-c.pid, 'SIGTERM');
    } catch {
      try {
        c.kill('SIGTERM');
      } catch {
        /* gone */
      }
    }
  }
};
process.on('SIGINT', () => {
  stop();
  process.exit(0);
});
process.on('SIGTERM', () => {
  stop();
  process.exit(0);
});
process.on('exit', stop);

function run(name, cmd, args, opts) {
  const log = fs.openSync(path.join(runDir, `${name}.log`), 'a');
  const c = spawn(cmd, args, { stdio: ['ignore', log, log], detached: true, ...opts });
  children.push(c);
  fs.writeFileSync(pidFile, JSON.stringify(children.map((x) => x.pid)));
  c.on('exit', (code) => {
    console.error(`[sdlc-e2e] ${name} exited with ${code}; see ${path.join(runDir, `${name}.log`)}`);
    stop();
    process.exit(1);
  });
  return c;
}

async function waitHttp(url, ms = 180000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      const r = await fetch(url);
      if (r.status < 500) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`timeout waiting for ${url}`);
}

// 1. fake GitLab
run('fake-gitlab', 'python3', [path.join(here, 'fake_gitlab.py'), String(gitlabPort), path.join(runDir, 'git')]);
await waitHttp(`http://127.0.0.1:${gitlabPort}/__state`, 20000);
const seeded = await fetch(`http://127.0.0.1:${gitlabPort}/__seed?ns=team&repo=app`, { method: 'POST' });
if (!seeded.ok) throw new Error(`seed failed: ${await seeded.text()}`);

// 2. web-platform (build first so startup time is bounded)
const build = spawnSync('cargo', ['build', '-p', 'web-platform'], { cwd: repoRoot, stdio: 'inherit' });
if (build.status !== 0) throw new Error('cargo build -p web-platform failed');
const bin = path.join(repoRoot, 'target', 'debug', 'web-platform');
const codexLog = path.join(runDir, 'codex.log');
run('web-platform', bin, [], {
  cwd: runDir,
  env: {
    PATH: process.env.PATH,
    HOME: process.env.HOME,
    RUST_LOG: process.env.RUST_LOG ?? 'info,web_platform=debug',
    JWT_SECRET: 'sdlc-e2e-jwt-secret-key-at-least-32-chars',
    ENCRYPTION_KEY: 'MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=',
    ADMIN_INIT_PASSWORD: 'admin123',
    DATABASE_URL: path.join(runDir, 'e2e.db'),
    SERVER_HOST: '127.0.0.1',
    SERVER_PORT: String(backendPort),
    SYMPHONY_WORKSPACE_ROOT: path.join(runDir, 'ws'),
    SYMPHONY_BIN: '/usr/bin/false',
    CORS_ORIGIN: `http://localhost:${webPort}`,
    SYMPHONY_DOC_AGENT_COMMAND: `FAKE_CODEX_LOG=${codexLog} python3 ${path.join(here, 'fake_codex.py')}`,
    NO_PROXY: '127.0.0.1,localhost',
    no_proxy: '127.0.0.1,localhost',
  },
});
await waitHttp(`http://127.0.0.1:${backendPort}/health`);
console.log(`[sdlc-e2e] stack ready: backend :${backendPort}, fake GitLab :${gitlabPort}, run dir ${runDir}`);
// Keep running until Playwright stops us; Playwright may SIGKILL us, so also
// exit (and stop children) once the parent process is gone.
const parent = process.ppid;
setInterval(() => {
  try {
    process.kill(parent, 0);
  } catch {
    stop();
    process.exit(0);
  }
  if (process.ppid !== parent) {
    stop();
    process.exit(0);
  }
}, 1000);
