import path from 'path';
import os from 'os';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** Isolated ports / paths for the SDLC E2E run (never 3000 / 5177). */
export const SDLC_E2E = {
  backendPort: Number(process.env.SDLC_E2E_BACKEND_PORT ?? 3917),
  gitlabPort: Number(process.env.SDLC_E2E_GITLAB_PORT ?? 18929),
  webPort: Number(process.env.SDLC_E2E_WEB_PORT ?? 5197),
  runDir: process.env.SDLC_E2E_RUN_DIR ?? path.join(os.tmpdir(), 'symphony-sdlc-e2e'),
  repoRoot: path.resolve(__dirname, '../../..'),
  here: __dirname,
  ns: 'team',
  repo: 'app',
  gitlabToken: 'e2e-gitlab-token',
};

export const gitlabUrl = () => `http://127.0.0.1:${SDLC_E2E.gitlabPort}`;
export const backendUrl = () => `http://127.0.0.1:${SDLC_E2E.backendPort}`;
export const webUrl = () => `http://localhost:${SDLC_E2E.webPort}`;
