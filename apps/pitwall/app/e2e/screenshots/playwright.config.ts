import { defineConfig, devices } from '@playwright/test';
import { e2ePortFor, reuseServer } from '../../../../../scripts/e2e-guard.cjs';

// Captures every screen in dark and light into docs/screenshots (npm run screenshots): 1440x900 logical, 2x pixels, one worker.
const PORT = e2ePortFor('PW_SHOT_PORT', 4390);

export default defineConfig({
  // Fails fast when ui/dist or the generated css is older than its sources (scripts/e2e-guard.cjs).
  globalSetup: '../../../../../scripts/e2e-guard-apps.cjs',
  testDir: '.',
  testMatch: '*.shot.ts',
  workers: 1,
  reporter: [['list']],
  use: { baseURL: `http://localhost:${PORT}`, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }],
  webServer: {
    command: `npx ng serve --port ${PORT} --watch=false`,
    cwd: '../..',
    url: `http://localhost:${PORT}`,
    reuseExistingServer: reuseServer(),
    timeout: 180_000,
  },
});
