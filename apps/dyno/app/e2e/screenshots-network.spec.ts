import { mkdirSync } from 'node:fs';
import { dialogWindowUrl, test } from '@decryp7/paddock-e2e';

// The map, the IP lists and the firewall page, light and dark, in their states, for the independent review (rule 22). Only on request:
// `SCREENSHOTS=1 NETWORK_SHOTS_DIR=<dir> PW_PORT=<port> npx playwright test --project=screenshots e2e/screenshots-network.spec.ts`.
const OUT = process.env['NETWORK_SHOTS_DIR'] ?? '';

test.describe('Dyno network screenshots', () => {
  test.skip(!OUT, 'set NETWORK_SHOTS_DIR');
  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      const shot = async (page: import('@playwright/test').Page, name: string) => {
        mkdirSync(OUT, { recursive: true });
        await page.waitForTimeout(800);
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png`, fullPage: true });
      };

      test('map', async ({ page }) => {
        await page.goto('/?latency=0#/map');
        await page.getByTestId('world-map').waitFor();
        await shot(page, 'map');
      });

      test('map, table view', async ({ page }) => {
        await page.goto('/?latency=0#/map');
        await page.getByRole('radio', { name: 'Table' }).click();
        await page.getByTestId('country-table').waitFor();
        await shot(page, 'map-table');
      });

      test('map, a country focused with the keyboard', async ({ page }) => {
        await page.goto('/?latency=0#/map');
        await page.getByRole('button', { name: /^United States: 54 connections/ }).focus();
        await shot(page, 'map-focus');
      });

      test('map, error with Retry', async ({ page }) => {
        await page.goto('/?latency=0&fail=geoSummary#/map');
        await page.getByRole('alert').first().waitFor();
        await shot(page, 'map-error');
      });

      test('connections filtered to a country', async ({ page }) => {
        await page.goto('/?latency=0#/map');
        await page.getByRole('button', { name: /^United States: 54 connections/ }).focus();
        await page.keyboard.press('Enter');
        await page.getByTestId('country-filter').waitFor();
        await shot(page, 'network-country-filter');
      });

      test('lists', async ({ page }) => {
        await page.goto('/?latency=0#/lists');
        await page.getByTestId('list-list-1').waitFor();
        await shot(page, 'lists');
      });

      test('lists, subscription form with the terms', async ({ page }) => {
        await page.goto('/?latency=0#/lists');
        await page.getByTestId('kind-url').check();
        await page.getByTestId('terms').waitFor();
        await shot(page, 'lists-subscribe');
      });

      test('lists, Remove duplicates preview', async ({ page }) => {
        await page.goto('/?latency=0#/lists');
        await page.getByLabel('New list name').fill('Messy');
        await page.getByLabel('New list entries').fill('8.8.8.8\n8.8.0.0/16\n8.8.8.8\nx.example\ny.x.example');
        await page.getByTestId('add-list').click();
        await page.getByTestId('list-list-2').getByTestId('dedupe').click();
        await page.getByTestId('preview').waitFor();
        await shot(page, 'lists-dedupe-preview');
      });

      test('lists, a refused subscription address', async ({ page }) => {
        await page.goto('/?latency=0#/lists');
        await page.getByTestId('kind-url').check();
        await page.getByLabel('New list name').fill('Feed');
        await page.getByLabel('List address').fill('http://lists.example/feed.txt');
        await page.getByTestId('add-list').click();
        await page.getByTestId('edit-error').waitFor();
        await shot(page, 'lists-refused');
      });

      test('lists, empty', async ({ page }) => {
        await page.goto('/?latency=0#/lists');
        await page.getByTestId('list-list-1').getByRole('button', { name: 'Remove', exact: true }).click();
        await page.getByTestId('confirm-remove').click();
        await page.getByTestId('no-lists').waitFor();
        await shot(page, 'lists-empty');
      });

      test('lists, error with Retry', async ({ page }) => {
        await page.goto('/?latency=0&fail=intelStatus#/lists');
        await page.getByRole('alert').first().waitFor();
        await shot(page, 'lists-error');
      });

      test('firewall', async ({ page }) => {
        await page.goto('/?latency=0#/firewall');
        await page.locator('[data-source="pf"]').waitFor();
        await shot(page, 'firewall');
      });

      test('firewall, error with Retry', async ({ page }) => {
        await page.goto('/?latency=0&fail=firewall#/firewall');
        await page.getByRole('alert').first().waitFor();
        await shot(page, 'firewall-error');
      });

      test('settings, Online data', async ({ page }) => {
        await page.setViewportSize({ width: 880, height: 700 });
        await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await page.getByText('Dyno', { exact: true }).first().click();
        const section = page.locator('section#online-data');
        await section.scrollIntoViewIfNeeded();
        await section.waitFor();
        mkdirSync(OUT, { recursive: true });
        await page.waitForTimeout(800);
        await section.screenshot({ path: `${OUT}/settings-online-data-${scheme}.png` });
      });
    });
  }
});
