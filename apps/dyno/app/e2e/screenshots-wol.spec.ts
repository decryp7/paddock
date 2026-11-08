import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Screenshots of every Wake-on-LAN state of the Power page's Wake devices tab and of Settings > Power > Wake-on-LAN, in both
 * themes, for the independent review of the UI (`SCREENSHOTS=1 npx playwright test --project=screenshots screenshots-wol`).
 * The mock sends nothing: its hardware addresses are invented and its networks are documentation ranges.
 */
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

type Step = (page: Page) => Promise<void>;

const wakeTab: Step = async (page) => {
  await page.getByRole('tab', { name: 'Wake devices' }).click();
  await expect(page.getByLabel("This computer's Wake-on-LAN readiness")).toBeVisible();
  await page.clock.runFor(500);
};

const SHOTS: { name: string; query: string; steps?: Step }[] = [
  { name: 'list', query: '' },
  { name: 'empty', query: 'wol=empty' },
  { name: 'off', query: 'wol=off' },
  { name: 'cant-send', query: 'wol=cantSend' },
  { name: 'readiness-cant-tell', query: 'wol=readinessCantTell' },
  {
    name: 'form-add',
    query: '',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add by hand' }).click();
      await expect(page.getByLabel('Computer to wake')).toBeVisible();
    },
  },
  {
    name: 'form-error',
    query: '',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add by hand' }).click();
      await page.getByLabel('Name of the computer').fill('Garage PC');
      await page.getByLabel('Hardware address').fill('nope');
      await page.getByRole('button', { name: 'Add computer' }).click();
      await expect(page.locator('mat-error')).toContainText('six pairs');
    },
  },
  {
    name: 'form-edit',
    query: '',
    steps: async (page) => {
      await page.locator('[role=row][data-key="2"]').click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Rename or change' }).click();
      await expect(page.getByLabel('Computer to wake')).toBeVisible();
    },
  },
  {
    name: 'network-list',
    query: '',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add from network' }).click();
      await expect(page.getByLabel('Computers not saved yet')).toBeVisible();
    },
  },
  {
    name: 'network-empty',
    query: 'wol=noNeighbours',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add from network' }).click();
      await expect(page.getByText('No other computers seen yet')).toBeVisible();
    },
  },
  {
    name: 'network-error',
    query: 'wol=neighboursFail',
    steps: async (page) => {
      await page.getByRole('button', { name: 'Add from network' }).click();
      await expect(page.getByLabel('Computers on your network').getByRole('alert')).toBeVisible();
    },
  },
  {
    name: 'wake-sent',
    query: '',
    steps: async (page) => {
      await page.locator('[role=row][data-key="1"]').getByRole('button', { name: 'Wake Media PC' }).click();
      await expect(page.getByLabel('Wake of Media PC')).toContainText('Wake packets sent');
    },
  },
  {
    name: 'test-answered',
    query: '',
    steps: async (page) => {
      await page.locator('[role=row][data-key="1"]').click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Test wake' }).click();
      await expect(page.getByLabel('Wake of Media PC')).toContainText('Awake after 23 s');
    },
  },
  {
    name: 'test-timeout',
    query: 'wol=timeout',
    steps: async (page) => {
      await page.locator('[role=row][data-key="1"]').click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Test wake' }).click();
      await expect(page.getByLabel('Wake of Media PC')).toContainText('No answer in 120 s');
    },
  },
  {
    name: 'test-running',
    query: 'jobsteps=40&latency=1',
    steps: async (page) => {
      await page.locator('[role=row][data-key="1"]').click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Test wake' }).click();
      await expect(page.getByLabel('Wake of Media PC')).toContainText('Test wake: Media PC');
    },
  },
];

for (const theme of ['light', 'dark'] as const) {
  for (const shot of SHOTS) {
    test(`wol ${shot.name} ${theme}`, async ({ page }) => {
      await page.clock.install({ time: FROZEN });
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 1280, height: 1100 });
      await page.goto(`/?${shot.query}#/power`);
      await wakeTab(page);
      await shot.steps?.(page);
      await page.clock.runFor(300);
      mkdirSync(OUT, { recursive: true });
      await page.screenshot({ path: join(OUT, `wol-${shot.name}-${theme}.png`) });
    });
  }
  test(`wol settings ${theme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.setViewportSize({ width: 880, height: 2600 });
    await page.goto(`/#/dialog?id=pd.settings&host=main&label=d-main-1&title=Settings&dialogInput=${encodeURIComponent('{}')}`);
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
    await page.getByText('Dyno', { exact: true }).first().click();
    const power = page.locator('section#power');
    await power.scrollIntoViewIfNeeded();
    await expect(power.getByText('Allow Wake-on-LAN')).toBeVisible();
    mkdirSync(OUT, { recursive: true });
    await power.scrollIntoViewIfNeeded();
    await power.screenshot({ path: join(OUT, `wol-settings-${theme}.png`) });
  });
}
