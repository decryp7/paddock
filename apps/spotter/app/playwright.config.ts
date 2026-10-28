import { defineConfig, devices } from '@playwright/test';
import { e2ePortFor, reuseServer } from '../../../scripts/e2e-guard.cjs';

/**
 * Browser end-to-end flows against `ng serve` with the mock backend (`npx playwright test --workers=1`), and the
 * documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`, written to ../docs/screenshots).
 * Port 4370, never the default 4200.
 */
const PORT = e2ePortFor('PW_PORT', 4370);

export default defineConfig({
  // Fails fast when ui/dist or the generated css is older than its sources (scripts/e2e-guard.cjs).
  globalSetup: '../../../scripts/e2e-guard-apps.cjs',
  testDir: './e2e',
  // Inside .angular/, which git ignores.
  outputDir: './.angular/playwright-results',
  fullyParallel: false,
  workers: 1,
  retries: process.env['CI'] ? 1 : 0,
  reporter: 'list',
  timeout: 60_000,
  use: {
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'chromium', testIgnore: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 } } },
    { name: 'webkit', testIgnore: /screenshots\.spec\.ts/, use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 } } },
    // Only on request, so a plain `npx playwright test` never rewrites the documentation images.
    ...(process.env['SCREENSHOTS']
      ? [{ name: 'screenshots', testMatch: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }]
      : []),
  ],
  webServer: {
    command: `npx ng serve --port ${PORT}`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: reuseServer(),
    timeout: 180_000,
  },
});
