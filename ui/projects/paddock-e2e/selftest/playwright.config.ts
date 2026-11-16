import { paddockConfig } from '../src/index.ts';

/**
 * The kit tests itself with its own preset on `data:` pages, so no server and no app are needed: the documentation
 * screenshot runner (`npx playwright test -c projects/paddock-e2e/selftest/playwright.config.ts --project=screenshots`) and
 * the helpers whose behaviour differs by engine (`--project=chromium --project=webkit`).
 */
export default paddockConfig({
  app: 'paddock-e2e-selftest',
  defaultPort: 4399,
  testDir: '.',
  env: { ...process.env, SCREENSHOTS: '1' },
  extend: { outputDir: '../../../.angular/playwright-selftest' },
});
