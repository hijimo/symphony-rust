import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

/**
 * Global setup for E2E tests.
 * Removes the SQLite database to ensure a fresh state for each test run.
 */
export default function globalSetup() {
  const dbPath = path.resolve(__dirname, '../../data.db');
  if (fs.existsSync(dbPath)) {
    fs.unlinkSync(dbPath);
    console.log('[global-setup] Removed existing data.db');
  } else {
    console.log('[global-setup] No existing data.db found, starting fresh');
  }

  // Ensure auth storage directory exists
  const authDir = path.resolve(__dirname, '.auth');
  if (!fs.existsSync(authDir)) {
    fs.mkdirSync(authDir, { recursive: true });
  }
}
