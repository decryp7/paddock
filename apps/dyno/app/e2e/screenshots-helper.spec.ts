import { expect } from './test';
import type { Page } from '@playwright/test';
import { join } from 'node:path';
import { captureDocScreens, dialogWindowUrl, settleTransitions, type DocScreen } from '@decryp7/paddock-e2e';

// The Administrator readings card in every state, light and dark, in the helper folder of the documentation screenshots, through the shared runner
// (`SCREENSHOTS=1 npx playwright test --project=screenshots -g "documentation screenshots"`): the pinned clock, live updates stopped, no animation.
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const STATES: readonly [string, string][] = [
  ['off', 'off'],
  ['silent', 'silent'],
  ['foreign', 'foreign'],
  ['outdated', 'outdated'],
  ['ready', 'ready'],
  ['not-included', 'notIncluded'],
];

const card = (page: Page) => page.getByRole('region', { name: 'Administrator readings' });

/** Opens Settings (880 x 760) with the mock's query and shows Dyno's page of it, where the card is. */
async function openCard(page: Page, query: string): Promise<void> {
  await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query }));
  await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  await page.getByText('Dyno', { exact: true }).first().click();
}

const screens: DocScreen[] = [
  ...STATES.map(([name, helper]): DocScreen => ({
    name: `helper/card-${name}`,
    url: 'about:blank',
    size: { width: 880, height: 760 },
    ready: async (page) => {
      await openCard(page, `?latency=0&helper=${helper}`);
      await expect(card(page).locator('pd-status-pill')).toBeVisible();
    },
    element: (page) => card(page),
  })),
  ...(['windows', 'linux'] as const).map(
    (os): DocScreen => ({
      name: `helper/card-off-${os}`,
      url: 'about:blank',
      size: { width: 880, height: 760 },
      ready: async (page) => {
        await openCard(page, `?latency=0&mock=${os}&helper=off`);
        await expect(card(page).locator('pd-status-pill')).toBeVisible();
      },
      element: (page) => card(page),
    }),
  ),
  {
    // Mid-job: the prompt phase cannot be cancelled.
    name: 'helper/card-turning-on',
    url: 'about:blank',
    size: { width: 880, height: 760 },
    ready: async (page) => {
      await openCard(page, '?latency=400&helper=off&jobsteps=16');
      await card(page).getByRole('button', { name: 'Turn on administrator readings' }).click();
      await expect(card(page).locator('pd-status-pill')).toContainText('Turning on');
      // The job is at its permission prompt (the phase the picture is about), and the card has stopped moving.
      await expect(card(page)).toContainText("Waiting for the system's permission prompt");
      await settleTransitions(page);
    },
    element: (page) => card(page),
  },
];

captureDocScreens({ screens, outDir: OUT, layout: 'flat', timezoneId: 'UTC', locale: 'en-GB' });
