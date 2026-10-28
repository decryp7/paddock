import { defineConfig, devices } from '@playwright/test';
import { e2ePortFor, reuseServer } from '../../../../../scripts/e2e-guard.cjs';

// Visual QA sweep: screenshots every view, menu and dialog of the browser build (mock backend) in both themes and at
// several window sizes, and reports layout problems (overlapping controls, clipped text). Not part of the e2e gate.
// Run from app/: npx playwright test -c scripts/visual-qa/playwright.config.ts
// Output: ../target/visual-qa (override with DEBRIEF_QA_OUT). Pick sizes/themes with DEBRIEF_QA_SIZES=1600x1000,1024x700 and
// DEBRIEF_QA_THEMES=dark,light.
const PORT = e2ePortFor('DEBRIEF_QA_PORT', 4330);

export default defineConfig({
  // Fails fast when ui/dist or the generated css is older than its sources (scripts/e2e-guard.cjs).
  globalSetup: '../../../../../scripts/e2e-guard-apps.cjs',
  testDir: '.',
  testMatch: 'sweep.ts',
  workers: e2ePortFor('DEBRIEF_QA_WORKERS', 2),
  fullyParallel: true,
  timeout: 900_000,
  reporter: [['list']],
  use: {
    ...devices['Desktop Chrome'],
    baseURL: `http://localhost:${PORT}`,
    deviceScaleFactor: 1,
    actionTimeout: 8_000,
    navigationTimeout: 30_000,
  },
  webServer: {
    command: `npx ng serve --port ${PORT}`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: reuseServer(),
    timeout: 180_000,
  },
});
