import { paddockConfig } from '../src/index.ts';

/**
 * The kit tests itself with its own preset (`npx playwright test -c projects/paddock-e2e/selftest/playwright.config.ts
 * --project=screenshots`; the other specs run in chromium and webkit with no project flag): the documentation screenshot runner and the
 * pinned clock on `data:` pages, so no server and no app are needed.
 */
export default paddockConfig({
  app: 'paddock-e2e-selftest',
  defaultPort: 4399,
  testDir: '.',
  webkit: true,
  env: { ...process.env, SCREENSHOTS: '1' },
  extend: { outputDir: '../../../.angular/playwright-selftest' },
});
