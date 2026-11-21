import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard, reuseServer } from '../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { paddockServers } from '../../../ui/projects/paddock-e2e/src/servers.ts';
import { assertChromeForTesting } from '../../../ui/projects/paddock-e2e/src/browser.ts';
import { internalBrowserUse } from '../../../ui/projects/paddock-e2e/src/browser-launcher.ts';
import { e2ePortOrCiDefault } from '../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, './'), resolve(__dirname, '../../../ui'));

/**
 * Browser end-to-end flows against `ng serve` with the mock backend (`npx playwright test --workers=1`), and the
 * documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`, written to ../docs/screenshots).
 * Port 4370, never the default 4200.
 */
const PORT = e2ePortOrCiDefault('PW_PORT', 4370);

// The guard refuses an installed browser (rule 7) and a browser folder on a removable volume (ER-14).
export default assertChromeForTesting(defineConfig({
  testDir: './e2e',
  // Inside .angular/, which git ignores.
  outputDir: './.angular/playwright-results',
  fullyParallel: false,
  workers: 1,
  // ER-12: a test that needed its one retry fails the run (the report marks it flaky), so a retry never turns red into green.
  retries: process.env['CI'] ? 1 : 0,
  failOnFlakyTests: !!process.env['CI'],
  reporter: 'list',
  timeout: 60_000,
  use: {
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'chromium', testIgnore: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1280, height: 800 } } },
    { name: 'webkit', testIgnore: /screenshots\.spec\.ts/, use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 } } },
    // Only on request, so a plain `npx playwright test` never rewrites the documentation images.
    ...(process.env['SCREENSHOTS']
      ? [{ name: 'screenshots', testMatch: /screenshots\.spec\.ts/, use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }]
      : []),
  ],
  ...paddockServers([
    {
      command: `npx ng serve --port ${PORT}`,
      url: `http://localhost:${PORT}`,
      reuseExisting: reuseServer(),
      timeoutMs: 180_000,
    },
  ]),
}));
