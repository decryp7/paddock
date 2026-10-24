import { defineConfig, devices } from '@playwright/test';
import { assertChromeForTesting, e2ePort } from './projects/paddock-e2e/src/index.ts';

/**
 * Browser checks of the reference apps (`npx playwright test`): the gallery (`ng serve gallery`) and the shell demo
 * (`ng serve shell-demo`), and the documentation screenshots (`SCREENSHOTS=1 npx playwright test`, written to
 * ../docs/screenshots). Ports come from PW_PORT and SHELL_DEMO_PORT (one pair per worktree; CI must set them). The
 * browser is Playwright's Chromium (Chrome for Testing), never the installed Chrome (rule 7, @decryp7/paddock-e2e).
 */
const PORT = e2ePort('PW_PORT', 4300);
const SHELL_DEMO_PORT = e2ePort('SHELL_DEMO_PORT', 4301);

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
      { name: 'gallery', testMatch: ['gallery.spec.ts', 'data.spec.ts'], use: { baseURL: `http://localhost:${PORT}` } },
      { name: 'review', testMatch: ['contact-sheet.spec.ts', 'contact-sheet-x21.spec.ts'] },
      { name: 'shell-demo', testMatch: /shell-demo(-[a-z0-9]+)?\.spec\.ts$/, use: { baseURL: `http://localhost:${SHELL_DEMO_PORT}` } },
    ],
    webServer: [
      {
        command: `npm run build:css && npx ng serve gallery --port ${PORT}`,
        url: `http://localhost:${PORT}`,
        reuseExistingServer: true,
        timeout: 180_000,
      },
      {
        command: `npm run build:css && npx ng serve shell-demo --port ${SHELL_DEMO_PORT}`,
        url: `http://localhost:${SHELL_DEMO_PORT}`,
        reuseExistingServer: true,
        timeout: 180_000,
      },
    ],
  }),
);
