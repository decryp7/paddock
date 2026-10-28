import { defineConfig, devices } from '@playwright/test';

/**
 * Browser end-to-end flows against `ng serve` with the mock backend (`npx playwright test --workers=1`), and the
 * documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`, written to ../docs/screenshots).
 * The dev server runs on port 4352 (not 4200) so it never clashes with another app on a shared machine.
 */
const PORT = Number(process.env['PW_PORT'] ?? 4352);

export default defineConfig({
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
    { name: 'chromium', testIgnore: /screenshots.*\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 } } },
    { name: 'webkit', testIgnore: /screenshots.*\.spec\.ts/, use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 } } },
    // Only on request, so a plain `npx playwright test` never rewrites the documentation images.
    ...(process.env['SCREENSHOTS']
      ? [{ name: 'screenshots', testMatch: /screenshots.*\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }]
      : []),
  ],
  webServer: {
    command: `npx ng serve --port ${PORT} --poll 1500`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: process.env['PW_REUSE'] === '1',
    timeout: 180_000,
  },
});
