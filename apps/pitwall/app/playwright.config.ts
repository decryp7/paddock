import { defineConfig, devices } from '@playwright/test';
import { e2ePortFor, reuseServer } from '../../../scripts/e2e-guard.cjs';

// End-to-end tests run against the browser build, where the app injects the in-memory MockBackend.
const PORT = e2ePortFor('PW_E2E_PORT', 4350);
const CI = !!process.env['CI'];

export default defineConfig({
  // Fails fast when ui/dist or the generated css is older than its sources (scripts/e2e-guard.cjs).
  globalSetup: '../../../scripts/e2e-guard-apps.cjs',
  testDir: 'e2e',
  testMatch: '**/*.e2e.ts',
  fullyParallel: true,
  forbidOnly: CI,
  retries: CI ? 2 : 0,
  workers: 1,
  outputDir: './.angular/playwright-results',
  reporter: [['list']],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: 'on-first-retry',
    viewport: { width: 1440, height: 900 },
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } } },
  ],
  webServer: {
    // No file watching: an edit made while the suite runs would reload open pages mid-test.
    command: `npx ng serve --port ${PORT} --watch=false`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: reuseServer(),
    timeout: 180_000,
  },
});
