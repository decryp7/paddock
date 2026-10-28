import { defineConfig, devices } from '@playwright/test';
import { e2ePortFor, reuseServer } from '../../../../../scripts/e2e-guard.cjs';

// Regenerates the README screenshots (docs/screenshots) from the browser build with the mock backend.
// Run from app/: npm run screenshots   (needs `npm run e2e:install` once for the Chromium browser)
const PORT = e2ePortFor('DEBRIEF_SHOT_PORT', 4320);

export default defineConfig({
  // Fails fast when ui/dist or the generated css is older than its sources (scripts/e2e-guard.cjs).
  globalSetup: '../../../../../scripts/e2e-guard-apps.cjs',
  testDir: '.',
  testMatch: 'capture.ts',
  workers: 1,
  timeout: 900_000,
  reporter: [['list']],
  use: {
    ...devices['Desktop Chrome'],
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    actionTimeout: 8_000,
    navigationTimeout: 30_000,
  },
  webServer: {
    command: `node scripts/build-info.mjs serve --port ${PORT}`, // the commit and date About shows
    cwd: process.cwd(),
    url: `http://localhost:${PORT}`,
    reuseExistingServer: reuseServer(),
    timeout: 180_000,
  },
});
