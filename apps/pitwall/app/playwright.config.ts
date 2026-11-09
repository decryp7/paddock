import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard } from '../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { e2ePortOrCiDefault } from '../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, './'), resolve(__dirname, '../../../ui'));

// End-to-end tests run against the browser build, where the app injects the in-memory MockBackend. Every project uses
// one production build served statically with the Tauri CSP header (ui/projects/paddock-e2e/src/serve-prod.ts), not the
// `ng serve` dev server: a dev server compiles and serves every module on request, and one stalled module request
// (@angular_material_radio.js, CI run 7854) left a page blank. A static bundle removes that class of failure (ER-12).
const PORT = e2ePortOrCiDefault('PW_E2E_PORT', 4350);
const CI = !!process.env['CI'];
// PW_E2E_PREBUILT=1 serves the dist/app/browser an earlier `npx ng build` left (CI builds once per job before the tests);
// otherwise the run builds first, so it never tests a stale build.
const PREBUILT = !!process.env['PW_E2E_PREBUILT'];
const SERVE = `node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON ../../../ui/projects/paddock-e2e/src/serve-prod.ts . ${PORT}`;

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
    trace: 'retain-on-failure',
    viewport: { width: 1440, height: 900 },
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } } },
  ],
  webServer: {
    command: PREBUILT ? SERVE : `npx ng build && ${SERVE}`,
    url: `http://localhost:${PORT}`,
    // Never reuse whatever listens on the port: a stale server would test the wrong build.
    reuseExistingServer: false,
    timeout: 300_000,
  },
});
