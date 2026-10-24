import { defineConfig, devices } from '@playwright/test';

/**
 * Browser checks of the reference apps (`npx playwright test`): the gallery (`ng serve gallery`) and the shell demo
 * (`ng serve shell-demo`), and the documentation screenshots (`SCREENSHOTS=1 npx playwright test`, written to
 * ../docs/screenshots). Ports come from PW_PORT and SHELL_DEMO_PORT (one pair per worktree).
 */
const PORT = Number(process.env['PW_PORT'] ?? 4300);
const SHELL_DEMO_PORT = Number(process.env['SHELL_DEMO_PORT'] ?? 4301);

export default defineConfig({
  testDir: './e2e',
  outputDir: './.angular/playwright-results',
  fullyParallel: false,
  workers: 1,
  retries: process.env['CI'] ? 1 : 0,
  reporter: 'list',
  timeout: 120_000,
  use: {
    ...devices['Desktop Chrome'],
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
  },
  projects: [
    { name: 'gallery', testMatch: 'gallery.spec.ts', use: { baseURL: `http://localhost:${PORT}` } },
    { name: 'shell-demo', testMatch: 'shell-demo.spec.ts', use: { baseURL: `http://localhost:${SHELL_DEMO_PORT}` } },
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
});
