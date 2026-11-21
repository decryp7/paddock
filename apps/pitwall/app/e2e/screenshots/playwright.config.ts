import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'node:path';
import { appBuildGuard } from '../../../../../ui/projects/paddock-e2e/src/build-guard.ts';
import { paddockServers } from '../../../../../ui/projects/paddock-e2e/src/servers.ts';
import { internalBrowserUse } from '../../../../../ui/projects/paddock-e2e/src/browser.ts';
import { e2ePortOrCiDefault } from '../../../../../ui/projects/paddock-e2e/src/env.ts';

// Fails fast, before any server starts, when what the app consumes of ui (ui/dist, the css) is stale (build-guard.ts).
appBuildGuard(resolve(__dirname, '../../'), resolve(__dirname, '../../../../../ui'));

// Captures every screen in dark and light into docs/screenshots (npm run screenshots): 1440x900 logical, 2x pixels, one worker.
const PORT = e2ePortOrCiDefault('PW_SHOT_PORT', 4390);

export default defineConfig({
  testDir: '.',
  testMatch: '*.shot.ts',
  workers: 1,
  reporter: [['list']],
  use: { baseURL: `http://localhost:${PORT}`, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], ...internalBrowserUse(), viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }],
  ...paddockServers([
    {
      // The production build served statically with the Tauri CSP (as the e2e config does); see ../../playwright.config.ts.
      command: `npx ng build && node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON ../../../ui/projects/paddock-e2e/src/serve-prod.ts . ${PORT}`,
      cwd: '../..',
      url: `http://localhost:${PORT}`,
      reuseExisting: false,
      timeoutMs: 300_000,
    },
  ]),
});
