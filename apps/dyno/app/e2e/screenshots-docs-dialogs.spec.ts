import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { FIXTURES } from './dialog-fixtures';
import { dialogContracts, dialogWindowUrl, expect, test, type DialogContractEntry } from '@decryp7/paddock-e2e';

/**
 * The dialogs of the documentation, as the app shows them: each is its own window (family rule), so the picture is the window's
 * page at its contract width and fitted height, not an overlay on a page (`SCREENSHOTS=1 npx playwright test --project=screenshots`).
 * Settings is the shell's window at the size Rust opens it.
 */
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', deviceScaleFactor: 2 });

/** The fitted-content dialogs: what shows that the content (not the skeleton) is there, then the size the window fits to. */
const FITTED: { name: string; id: string; loaded: string }[] = [
  { name: 'web-search', id: 'dy.web-search', loaded: 'input[name=query]' },
  { name: 'hash-search', id: 'dy.file-hash', loaded: 'code[aria-label="SHA-256 hash"]' },
];

for (const scheme of ['dark', 'light'] as const) {
  test.describe(`documentation dialogs (${scheme})`, () => {
    test.use({ colorScheme: scheme });

    for (const d of FITTED) {
      test(`${d.name} (${scheme})`, async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        const contract = (CONTRACTS as Record<string, DialogContractEntry>)[d.id]!;
        await page.clock.install({ time: FROZEN });
        await page.setViewportSize({ width: contract.width, height: 800 });
        await page.goto(dialogWindowUrl(d.id, FIXTURES[d.id]![0] ?? {}, { title: d.id }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await expect(page.locator(d.loaded).first()).toBeVisible({ timeout: 15_000 });
        await page.clock.runFor(1_000);
        let last = -1;
        await expect(async () => {
          const h = await page.evaluate(() => Number(document.documentElement.dataset['fittedHeight'] ?? '0'));
          const previous = last;
          last = h;
          expect(h).toBe(previous);
        }).toPass({ timeout: 10_000, intervals: [200, 200, 300] });
        await page.setViewportSize({ width: contract.width, height: last });
        await expect(page.locator('[data-testid=dialog-footer]')).toBeVisible();
        await page.screenshot({ path: join(OUT, `${d.name}-${scheme}.png`) });
      });
    }

    // The Settings window, Dyno's page of it; the second picture has the menu bar items edited.
    for (const [name, height] of [['settings', 1100], ['settings-menu-bar', 1100]] as const) {
      test(`${name} (${scheme})`, async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.clock.install({ time: FROZEN });
        await page.setViewportSize({ width: 880, height });
        await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await page.getByText('Dyno', { exact: true }).first().click();
        await expect(page.getByRole('region', { name: 'Menu bar items' })).toBeVisible();
        if (name === 'settings-menu-bar') {
          const choose = async (label: string, option: string) => {
            await page.getByRole('combobox', { name: label }).click();
            await page.getByRole('option', { name: option, exact: true }).click();
          };
          const section = page.getByRole('region', { name: 'Menu bar items' });
          await section.getByRole('button', { name: 'Add item' }).click();
          await expect(section.getByRole('listitem')).toHaveCount(3);
          await section.getByRole('button', { name: 'Add item' }).click();
          await expect(section.getByRole('listitem')).toHaveCount(4);
          await choose('Style of item 1', 'Graph and number');
          await choose('Metric of item 2', 'Memory');
          await choose('Style of item 2', 'Ring');
          await choose('Metric of item 3', 'Network down');
          await choose('Style of item 3', 'Line graph');
          await choose('Metric of item 4', 'Temperature');
          await choose('Style of item 4', 'Number');
          await section.evaluate((el) => el.scrollIntoView({ block: 'start' }));
        }
        await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
        await page.mouse.move(2, 2);
        await page.clock.runFor(2_000);
        await page.screenshot({ path: join(OUT, `${name}-${scheme}.png`) });
      });
    }
  });
}
