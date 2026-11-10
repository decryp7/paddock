import { expect, test } from '@playwright/test';
import { openApp } from './app';

// ci.yml run 7854 (job 34010): a11y.e2e.ts "apps" failed in openApp with only "locator('h1') element(s) not found". The trace
// showed why: one module of the Apps page (then the dev server's pre-bundled @angular/material/radio) got no answer, so the
// lazy page never loaded and the shell kept its "Loading Apps & keys" placeholder. A screen that does not render must say
// which requests it is still waiting for (ER-27), so the next occurrence names its cause in the test report. The e2e run now
// serves the production bundle, so the stalled request here is the lazy chunk that holds the Apps page (found by its selector).
test('a page that never renders names the module requests that got no answer', async ({ page }) => {
  await page.route(/\/chunk-[^/]+\.js$/, async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    // Held, never answered: the request stays open as the stalled one did.
    if (body.includes('pw-apps-page')) return;
    await route.fulfill({ response, body });
  });
  const failure = await openApp(page, '/apps').then(
    () => null,
    (e: unknown) => (e instanceof Error ? e.message : String(e)),
  );
  expect(failure, 'openApp resolved although the Apps page could not load').not.toBeNull();
  expect(failure).toContain('no answer');
  expect(failure).toMatch(/chunk-[\w-]+\.js/);
  await expect(page.getByRole('status', { name: 'Loading Apps & keys' })).toBeVisible();
});
