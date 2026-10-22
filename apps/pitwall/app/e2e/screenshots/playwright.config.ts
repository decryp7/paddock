import { defineConfig, devices } from '@playwright/test';

// Captures every screen in dark and light into docs/screenshots (npm run screenshots): 1440x900 logical, 2x pixels, one worker.
const PORT = Number(process.env['PW_SHOT_PORT'] ?? 4390);

export default defineConfig({
  testDir: '.',
  testMatch: '*.shot.ts',
  workers: 1,
  reporter: [['list']],
  use: { baseURL: `http://localhost:${PORT}`, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }],
  webServer: {
    command: `npx ng serve --port ${PORT} --watch=false`,
    cwd: '../..',
    url: `http://localhost:${PORT}`,
    reuseExistingServer: true,
    timeout: 180_000,
  },
});
