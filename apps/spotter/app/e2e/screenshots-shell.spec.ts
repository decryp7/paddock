import { expect, test } from '@decryp7/paddock-e2e';

/**
 * The screens of Spotter on the shell for review (`SCREENSHOTS=1 SHOTS_OUT=<folder> npx playwright test --project=screenshots`):
 * the whole 1440x900 window at 2x, in both themes, never cropped. The main screens, the shell's parts (the Jobs list with a
 * running download and Cancel, a service chip that is down with its repair, the notification centre) and the dialog windows
 * (Settings with Spotter's page beside Appearance and Updates, the confirm decision, About, Updates and What's new).
 */
const OUT = process.env['SHOTS_OUT'] ?? '../docs/screenshots';
const NOW = new Date('2026-10-22T12:00:00Z');

for (const scheme of ['dark', 'light'] as const) {
  test.describe(`${scheme} shell screens`, () => {
    test.use({ colorScheme: scheme, timezoneId: 'UTC', locale: 'en-US', viewport: { width: 1440, height: 900 } });
    test.beforeEach(async ({ page }) => {
      await page.clock.setFixedTime(NOW);
    });

    async function shoot(page: import('@playwright/test').Page, name: string): Promise<void> {
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
    }

    const screens: [string, string, string][] = [
      ['dashboard', '/', 'Dashboard'],
      ['dashboard-empty', '/?mock=empty', 'Dashboard'],
      ['watches', '/#/watches', 'Watches'],
      ['watches-empty', '/?mock=empty#/watches', 'Watches'],
      ['watch-detail', '/#/watches/w-1', ''],
      ['watch-detail-upgrade', '/#/watches/w-6', ''],
      ['watch-detail-products', '/?mock=products#/watches/w-9', ''],
      ['wizard-source', '/#/watches/new', 'Add a watch'],
    ];
    for (const [name, url, heading] of screens) {
      test(`${name}`, async ({ page }) => {
        await page.goto(url);
        if (heading) await expect(page.getByRole('heading', { name: heading, level: 1 })).toBeVisible();
        else await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
        await expect(page.locator('pd-splash')).toHaveCount(0);
        await shoot(page, name);
      });
    }

    test('jobs list with a running download and Cancel', async ({ page, app }) => {
      await page.goto('/?jobStepMs=4000');
      await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Download' }).click();
      await app.statusBar.jobs.click();
      await expect(app.jobs.items.first()).toContainText(/MB of/);
      await shoot(page, 'jobs');
    });

    test('a service that is down, with its repair', async ({ page, app }) => {
      await page.goto('/?mock=broken');
      await expect(app.statusBar.chip).toContainText(/your sources/i);
      await app.statusBar.chip.click();
      await expect(app.notifications.services).toBeVisible();
      await shoot(page, 'health-down');
    });

    test('the notification centre', async ({ page, app }) => {
      await page.goto('/?mock=broken');
      await expect(app.statusBar.chip).toContainText(/your sources/i);
      await app.notifications.openCentre();
      await shoot(page, 'notification-centre');
    });

    test('the confirm decision', async ({ page, app }) => {
      await page.goto('/#/watches');
      await page.locator('tbody tr', { hasText: 'Lumen CLI' }).click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Delete watch' }).click();
      await expect(app.dialogs.inPage('Confirm')).toBeVisible();
      await shoot(page, 'confirm');
    });

    for (const [name, id] of [['settings', 'pd.settings'], ['about', 'pd.about'], ['updates', 'pd.updates'], ['whats-new', 'pd.whats-new']] as const) {
      test(`${name} window`, async ({ page, app }) => {
        await app.dialogs.openWindow(id);
        await app.dialogs.loaded();
        await shoot(page, name);
      });
    }
  });
}
