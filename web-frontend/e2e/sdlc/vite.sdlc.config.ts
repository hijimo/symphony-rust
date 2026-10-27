import { defineConfig, mergeConfig } from 'vite';
import base from '../../vite.config';

const backendPort = Number(process.env.SDLC_E2E_BACKEND_PORT ?? 3917);

/** Same as the app vite config, but /api (incl. WebSocket) → the isolated E2E backend. */
export default mergeConfig(
  base,
  defineConfig({
    server: {
      proxy: {
        '/api': { target: `http://127.0.0.1:${backendPort}`, changeOrigin: true, ws: true },
      },
    },
  }),
  false,
);
