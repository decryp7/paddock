import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard } from '../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { e2ePortOrCiDefault } from '../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, './'), resolve(__dirname, '../../../ui'));

// End-to-end tests run against the browser build, where the app injects the in-memory MockBackend. Every project uses
// the one production build, served with the Tauri CSP header (e2e/serve-prod.mjs): one build per run (and per CI shard)
// instead of a dev server plus a production build, and every spec runs under the CSP the desktop app enforces.
const CI = !!process.env['CI'];
// CI must name its port (a fixed default could collide with another job on the host); locally the default is derived from the worktree (offset of 4340, apart from the ui gallery's 4300).
if (CI && !process.env['DEBRIEF_E2E_PORT']) throw new Error('CI: set DEBRIEF_E2E_PORT (the port of the production-build server)');
const PORT = e2ePortOrCiDefault('DEBRIEF_E2E_PORT', 4340);
// DEBRIEF_E2E_PREBUILT=1 serves the dist/app/browser an earlier `npx ng build` left (CI builds once before the tests);
// otherwise the run builds first, so it never tests a stale build.
const PREBUILT = !!process.env['DEBRIEF_E2E_PREBUILT'];
// The CSP guard spec runs in Chromium only.
const PROD_CSP = '**/prod-csp.e2e.ts';
// The shell baseline (Paddock X50 W0, e2e/shell-baseline.md) is a manual migration baseline: macOS screenshots that the
// workbench migration (M5) diffs against on the same Mac. It is not part of the default run (CI runs Linux, whose fonts
// render differently); DEBRIEF_SHELL_BASELINE=1 adds its two projects (`npm run e2e:shell-baseline`).
const SHELL_BASELINE = '**/shell-baseline.e2e.ts';
const BASELINE = !!process.env['DEBRIEF_SHELL_BASELINE'];
// The baseline projects keep the snapshot names of the browser they run in (`<name>-chromium-darwin.png`).
const baselineSnapshots = (browser: string) => `{snapshotDir}/{testFileDir}/{testFileName}-snapshots/{arg}-${browser}{-platform}{ext}`;

export default defineConfig({
  testDir: 'e2e',
  testMatch: '**/*.e2e.ts',
  fullyParallel: true,
  forbidOnly: CI,
  // ER-12: at most one retry, and a test that needed it fails the run (the report marks it flaky), so a retry never turns red into green.
  retries: CI ? 1 : 0,
  failOnFlakyTests: true,
  // Multi-step flows (tour, live tail) outlast 30 s of wall clock on a shared WebKit runner without being broken.
  timeout: CI ? 60_000 : 30_000,
  // Two on CI: each shard's container shares a runner host with other jobs; three made WebKit tests time out.
  workers: CI ? 2 : undefined,
  reporter: [['html', { open: 'never' }], ['list'], ['./e2e/retried-reporter.mjs']],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: 'retain-on-failure',
    viewport: { width: 1440, height: 900 },
  },
  projects: [
    { name: 'chromium', testIgnore: SHELL_BASELINE, use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'webkit', testIgnore: [PROD_CSP, SHELL_BASELINE], use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } } },
    ...(BASELINE
      ? [
          {
            name: 'shell-baseline-chromium',
            testMatch: SHELL_BASELINE,
            snapshotPathTemplate: baselineSnapshots('chromium'),
            use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } },
          },
          {
            name: 'shell-baseline-webkit',
            testMatch: SHELL_BASELINE,
            snapshotPathTemplate: baselineSnapshots('webkit'),
            use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } },
          },
        ]
      : []),
  ],
  webServer: {
    command: PREBUILT ? `node e2e/serve-prod.mjs ${PORT}` : `npx ng build && node e2e/serve-prod.mjs ${PORT}`,
    url: `http://localhost:${PORT}`,
    // Never reuse whatever listens on the port: a stale server would test the wrong build.
    reuseExistingServer: false,
    timeout: 300_000,
  },
});
