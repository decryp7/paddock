import { defineConfig, devices } from '@playwright/test';
import { assertChromeForTesting, e2ePort, isOn, reuseServer, SCREENSHOT_SPECS, uiConfigGuard } from './projects/paddock-e2e/src/index.ts';

/**
 * Browser checks of the reference apps (`npx playwright test`): the gallery (`ng serve gallery`) and the shell demo
 * (`ng serve shell-demo`), and the documentation screenshots (`SCREENSHOTS=1 npx playwright test`, written to
 * docs/screenshots with hashes.json). Ports come from PW_PORT, SHELL_DEMO_PORT and WORKBENCH_DEMO_PORT; unset, they derive from the worktree's path (CI must set them: e2ePort refuses a default there).
 * A running server is reused only with PW_REUSE_SERVER=1; a stale ui/dist or css fails the run (rebuild ui: npm run build -w ui). The
 * browser is Playwright's Chromium (Chrome for Testing), never the installed Chrome (rule 7, @decryp7/paddock-e2e).
 *
 * WebKit (the engine of WKWebView, so what the Paddock apps render in on macOS): `PADDOCK_WEBKIT=1 npx playwright test
 * --project=gallery-webkit --project=shell-demo-webkit --project=workbench-demo-webkit`. These projects exist only when the
 * variable is set, so a plain run (and a branch run, rule 3) stays Chromium; the full gate before a build sets it.
 */
// A reused server does not rebuild the css (the webServers below do): then it must be fresh (build-guard.ts).
uiConfigGuard(__dirname);

const PORT = e2ePort('PW_PORT', 4300);
const SHELL_DEMO_PORT = e2ePort('SHELL_DEMO_PORT', 4301);
const WORKBENCH_DEMO_PORT = e2ePort('WORKBENCH_DEMO_PORT', 4302);

// The specs read the same variables, so they reach the derived ports too (workers inherit the environment).
process.env['PW_PORT'] ??= String(PORT);
process.env['SHELL_DEMO_PORT'] ??= String(SHELL_DEMO_PORT);
process.env['WORKBENCH_DEMO_PORT'] ??= String(WORKBENCH_DEMO_PORT);

const webkit = isOn(process.env['PADDOCK_WEBKIT']);
const safari = { ...devices['Desktop Safari'], viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 };

export default assertChromeForTesting(
  defineConfig({
    testDir: './e2e',
    outputDir: './.angular/playwright-results',
    fullyParallel: false,
    workers: 1,
    retries: process.env['CI'] ? 1 : 0,
    // Flakes (passed only after a retry) are listed, never hidden.
    reporter: [['list'], ['./projects/paddock-e2e/src/retried-reporter.ts']],
    timeout: 120_000,
    use: {
      ...devices['Desktop Chrome'],
      viewport: { width: 1280, height: 900 },
      deviceScaleFactor: 1,
    },
    projects: [
      { name: 'gallery', testMatch: ['gallery.spec.ts', 'data.spec.ts', 'view.spec.ts', 'material.spec.ts', 'grid-header.spec.ts', 'sunburst.spec.ts'], use: { baseURL: `http://localhost:${PORT}` } },
      { name: 'review', testMatch: ['contact-sheet.spec.ts', 'contact-sheet-x21.spec.ts', 'contact-sheet-x50.spec.ts', 'contact-sheet-x3x.spec.ts', 'contact-sheet-x51.spec.ts'] },
      // The documentation screenshots (e2e/screenshots.spec.ts, the kit's captureDocScreens): `SCREENSHOTS=1 npx playwright test --project=screenshots`.
      { name: 'screenshots', testMatch: SCREENSHOT_SPECS, use: { deviceScaleFactor: 2 } },
      { name: 'shell-demo', testMatch: /shell-demo(-[a-z0-9]+)?\.spec\.ts$/, use: { baseURL: `http://localhost:${SHELL_DEMO_PORT}` } },
      { name: 'workbench-demo', testMatch: /workbench-demo(-[a-z]+)?\.spec\.ts$/, use: { baseURL: `http://localhost:${WORKBENCH_DEMO_PORT}` } },
      ...(webkit
        ? [
            { name: 'gallery-webkit', testMatch: ['gallery.spec.ts', 'data.spec.ts', 'view.spec.ts', 'grid-header.spec.ts', 'sunburst.spec.ts'], use: { ...safari, baseURL: `http://localhost:${PORT}` } },
            { name: 'shell-demo-webkit', testMatch: /shell-demo(-[a-z0-9]+)?\.spec\.ts$/, use: { ...safari, baseURL: `http://localhost:${SHELL_DEMO_PORT}` } },
            { name: 'workbench-demo-webkit', testMatch: /workbench-demo(-[a-z]+)?\.spec\.ts$/, use: { ...safari, baseURL: `http://localhost:${WORKBENCH_DEMO_PORT}` } },
          ]
        : []),
    ],
    webServer: [
      {
        command: `npm run build:css && npx ng serve gallery --port ${PORT}`,
        url: `http://localhost:${PORT}`,
        reuseExistingServer: reuseServer(),
        timeout: 180_000,
      },
      {
        command: `npm run build:css && npx ng serve shell-demo --port ${SHELL_DEMO_PORT}`,
        url: `http://localhost:${SHELL_DEMO_PORT}`,
        reuseExistingServer: reuseServer(),
        timeout: 180_000,
      },
      {
        command: `npm run build:css && npx ng serve workbench-demo --port ${WORKBENCH_DEMO_PORT}`,
        url: `http://localhost:${WORKBENCH_DEMO_PORT}`,
        reuseExistingServer: reuseServer(),
        timeout: 180_000,
      },
    ],
  }),
);
