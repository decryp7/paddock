import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard, reuseServer } from '../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { e2ePortOrCiDefault } from '../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, './'), resolve(__dirname, '../../../ui'));

// End-to-end tests run against the browser build, where the app injects the in-memory MockBackend.
const PORT = e2ePortOrCiDefault('PW_E2E_PORT', 4350);
const CI = !!process.env['CI'];

export default defineConfig({
  testDir: 'e2e',
  testMatch: '**/*.e2e.ts',
  fullyParallel: true,
  forbidOnly: CI,
  // A flake is fixed, not retried (rule 5): no retries, in CI either.
  retries: 0,
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
