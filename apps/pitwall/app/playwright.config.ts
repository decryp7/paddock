import { defineConfig, devices } from '@playwright/test';

// End-to-end tests run against the browser build, where the app injects the in-memory MockBackend.
const PORT = Number(process.env['PW_E2E_PORT'] ?? 4350);
const CI = !!process.env['CI'];

export default defineConfig({
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
    reuseExistingServer: !CI,
    timeout: 180_000,
  },
});
