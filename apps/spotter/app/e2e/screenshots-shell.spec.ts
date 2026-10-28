import type { Page } from '@playwright/test';
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

    /** Waits for the screen to be what a user sees: no skeleton, the primary button drawn, fonts and avatar images decoded. */
    async function loaded(page: Page, options: { primary?: boolean } = {}): Promise<void> {
      await expect(page.locator('pd-skeleton')).toHaveCount(0);
      if (options.primary) await expect(page.locator('button.primary, .btn.primary').first()).toBeVisible();
      await page.evaluate(async () => {
        await document.fonts.ready;
        await Promise.all([...document.images].map((img) => img.decode().catch(() => undefined)));
      });
    }

    async function shoot(page: Page, name: string, options: { primary?: boolean } = {}): Promise<void> {
      await loaded(page, options);
      await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
    }

    const screens: [string, string, string, boolean][] = [
      ['dashboard', '/', 'Dashboard', true],
      ['dashboard-empty', '/?mock=empty', 'Dashboard', true],
      ['watches', '/#/watches', 'Watches', true],
      ['watches-empty', '/?mock=empty#/watches', 'Watches', true],
      ['watch-detail', '/#/watches/w-1', '', true],
      ['watch-detail-upgrade', '/#/watches/w-6', '', true],
      ['watch-detail-products', '/?mock=products#/watches/w-9', '', true],
      ['wizard-source', '/#/watches/new', 'Add a watch', false],
    ];
    for (const [name, url, heading, primary] of screens) {
      test(`${name}`, async ({ page }) => {
        await page.goto(url);
        if (heading) await expect(page.getByRole('heading', { name: heading, level: 1 })).toBeVisible();
        else await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
        await expect(page.locator('pd-splash')).toHaveCount(0);
        await shoot(page, name, { primary });
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

    // Dialog windows are shot at the size their definition declares (a fitted one at its width and the content's height), not
    // at the main window's: Settings 880x640, Updates 560x480, a viewer opens at 680x560 and only grows, a decision is 420 wide.
    const windows: [string, string, { width: number; height: number }, unknown][] = [
      ['settings', 'pd.settings', { width: 880, height: 640 }, {}],
      ['updates', 'pd.updates', { width: 560, height: 480 }, {}],
      ['about', 'pd.about', { width: 680, height: 560 }, {}],
      ['whats-new', 'pd.whats-new', { width: 680, height: 560 }, {}],
      ['confirm', 'sp.confirm', { width: 420, height: 160 }, { title: 'Delete watch', message: 'Delete "Lumen CLI"? Its history is deleted with it.', confirmLabel: 'Delete', danger: true }],
    ];
    for (const [name, id, size, input] of windows) {
      test(`${name} window`, async ({ page, app }) => {
        await page.setViewportSize(size);
        await app.dialogs.openWindow(id, input);
        await app.dialogs.loaded();
        await shoot(page, name);
      });
    }
  });
}
