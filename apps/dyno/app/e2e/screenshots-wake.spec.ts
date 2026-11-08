import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Screenshots of every state of the Power page's Schedule tab and of Settings > Power > Scheduled wake, in both themes, for the
 * independent review (`SCREENSHOTS=1 npx playwright test --project=screenshots screenshots-wake`). The mock sets no wake.
 */
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

type Step = (page: Page) => Promise<void>;

const scheduleTab: Step = async (page) => {
  await page.getByRole('tab', { name: 'Schedule' }).click();
  await expect(page.getByRole('region', { name: 'Scheduled wakes' })).toBeVisible();
  await page.clock.runFor(500);
};

const SHOTS: { name: string; query: string; steps?: Step }[] = [
  { name: 'list', query: 'wake=on' },
  { name: 'fresh-install-off', query: '' },
  { name: 'empty', query: 'wake=empty' },
    { name: 'needs-helper', query: 'wake=needsHelper' },
  { name: 'unavailable', query: 'wake=unavailable' },
  { name: 'agent-windows', query: 'wake=agent&mock=windows' },
  { name: 'row-failed', query: 'wake=failed' },
  { name: 'form-error-days', query: 'wake=on', steps: async (page) => {
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await page.getByLabel('Name of the schedule').fill('Weekdays');
      await page.getByRole('radio', { name: 'On chosen days' }).click();
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await expect(page.getByRole('alert')).toContainText('at least one day');
    } },
  {
    name: 'form-add',
    query: 'wake=on',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await expect(page.getByLabel('Scheduled wake', { exact: true })).toBeVisible();
    },
  },
  {
    name: 'form-weekdays',
    query: 'wake=on',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await page.getByRole('radio', { name: 'On chosen days' }).click();
      await page.getByRole('button', { name: 'Weekdays' }).click();
      await expect(page.getByRole('group', { name: 'Days of the week' })).toBeVisible();
    },
  },
  {
    name: 'form-error-name',
    query: 'wake=on',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await expect(page.locator('mat-error')).toContainText('Give the schedule a name');
    },
  },
  {
    name: 'form-error-choice',
    query: 'wake=on',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await page.getByLabel('Name of the schedule').fill('Nothing');
      await page.getByRole('checkbox', { name: 'Stay awake for' }).uncheck();
      await page.getByRole('button', { name: 'Add schedule' }).click();
      await expect(page.getByRole('alert')).toContainText('Choose what happens');
    },
  },
  {
    name: 'form-edit',
    query: 'wake=on',
    steps: async (page) => {
      await page.locator('[role=row][data-key="2"]').click();
      await expect(page.getByLabel('Scheduled wake', { exact: true })).toBeVisible();
    },
  },
];

for (const theme of ['light', 'dark'] as const) {
  for (const shot of SHOTS) {
    test(`wake ${shot.name} ${theme}`, async ({ page }) => {
      await page.addInitScript(() => localStorage.setItem('dyno.trayHintSeen', '1'));
      await page.clock.install({ time: FROZEN });
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 1280, height: 1100 });
      await page.goto(`/?${shot.query}#/power`);
      await scheduleTab(page);
      await shot.steps?.(page);
      await page.clock.runFor(300);
      // The Windows taskbar hint is a notification of the shell, not of this page: gone before the picture.
      await page.evaluate(() => document.querySelectorAll('pd-notification-host').forEach((e) => e.remove()));
      mkdirSync(OUT, { recursive: true });
      await page.screenshot({ path: join(OUT, `wake-${shot.name}-${theme}.png`) });
    });
  }
  test(`wake settings ${theme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.setViewportSize({ width: 880, height: 2600 });
    await page.goto(`/#/dialog?id=pd.settings&host=main&label=d-main-1&title=Settings&dialogInput=${encodeURIComponent('{}')}`);
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
    await page.getByText('Dyno', { exact: true }).first().click();
    const power = page.locator('section#power');
    await power.scrollIntoViewIfNeeded();
    await expect(power.getByText('Allow scheduled wake')).toBeVisible();
    mkdirSync(OUT, { recursive: true });
    await power.screenshot({ path: join(OUT, `wake-settings-${theme}.png`) });
  });
}
