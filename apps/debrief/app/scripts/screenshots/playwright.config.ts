import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard, reuseServer } from '../../../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { e2ePortOrCiDefault } from '../../../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, '../../'), resolve(__dirname, '../../../../../ui'));

// Regenerates the README screenshots (docs/screenshots) from the browser build with the mock backend.
// Run from app/: npm run screenshots   (needs `npm run e2e:install` once for the Chromium browser)
const PORT = e2ePortOrCiDefault('DEBRIEF_SHOT_PORT', 4320);

export default defineConfig({
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
