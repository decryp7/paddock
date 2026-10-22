import { defineConfig, devices } from '@playwright/test';

/**
 * Gallery checks against `ng serve gallery` (`npx playwright test`), and the documentation screenshots
 * (`SCREENSHOTS=1 npx playwright test`, written to ../docs/screenshots).
 */
const PORT = Number(process.env['PW_PORT'] ?? 4300);

export default defineConfig({
  testDir: './e2e',
  outputDir: './.angular/playwright-results',
  fullyParallel: false,
  workers: 1,
  retries: process.env['CI'] ? 1 : 0,
  reporter: 'list',
  timeout: 120_000,
  use: {
    baseURL: `http://localhost:${PORT}`,
    ...devices['Desktop Chrome'],
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
  },
  webServer: {
    command: `npm run build:css && npx ng serve gallery --port ${PORT}`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: true,
    timeout: 180_000,
  },
});
