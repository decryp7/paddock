import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard, reuseServer } from '../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { paddockServers } from '../../../ui/projects/paddock-e2e/src/servers.ts';
import { internalBrowserUse } from '../../../ui/projects/paddock-e2e/src/browser.ts';
import { e2ePortOrCiDefault } from '../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, './'), resolve(__dirname, '../../../ui'));

/**
 * Browser end-to-end flows against `ng serve` with the mock backend (`npx playwright test --workers=1`), and the
 * documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`, written to ../docs/screenshots).
 * The dev server runs on port 4352 (not 4200) so it never clashes with another app on a shared machine.
 */
const PORT = e2ePortOrCiDefault('PW_PORT', 4352);

export default defineConfig({
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
    { name: 'chromium', testIgnore: /screenshots.*\.spec\.ts/, use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1280, height: 800 } } },
    { name: 'webkit', testIgnore: /screenshots.*\.spec\.ts/, use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 } } },
    // Only on request, so a plain `npx playwright test` never rewrites the documentation images.
    ...(process.env['SCREENSHOTS']
      ? [{ name: 'screenshots', testMatch: /screenshots.*\.spec\.ts/, testIgnore: /screenshots-check\.spec\.ts/, use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }]
      : []),
    // Whether the tracked pictures are current (e2e/screenshots-check.spec.ts): regenerates the two documentation specs into SCREENSHOT_DIR, then
    // compares them with the tracked pictures by a pixel tolerance. `SCREENSHOTS_CHECK=1 SCREENSHOT_DIR=<scratch> npx playwright test --project=screenshots-check`.
    ...(process.env['SCREENSHOTS_CHECK']
      ? [
          {
            name: 'screenshots-made',
            testMatch: /screenshots(-docs-dialogs|-helper)?\.spec\.ts$/,
            use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 },
          },
          {
            name: 'screenshots-check',
            testMatch: /screenshots-check\.spec\.ts$/,
            dependencies: ['screenshots-made'],
            // The tracked picture is the expected one (<name>.png in the screenshots folder of apps/dyno/docs, found from the specs' folder); SCREENSHOTS_TRACKED names another folder (to measure).
            snapshotPathTemplate: `${process.env['SCREENSHOTS_TRACKED'] ?? '{snapshotDir}/../../docs/screenshots'}/{arg}{ext}`,
          },
        ]
      : []),
  ],
  updateSnapshots: 'none',
  ...paddockServers([
    {
      command: `npx ng serve --port ${PORT} --poll 1500`,
      url: `http://localhost:${PORT}`,
      reuseExisting: reuseServer(),
      timeoutMs: 180_000,
    },
  ]),
});
