import { expect, test } from '@decryp7/paddock-e2e';
import { loaded } from './loaded';

/**
 * The shell surfaces of Pitcrew for review (`SCREENSHOTS=1 SHOTS_OUT=<folder> npx playwright test --project=screenshots`): the
 * whole 1440x900 window at 2x, in both themes, never cropped. The shell's parts (the Jobs list with a running update and
 * Cancel, a service chip that is down with its Restart, the notification centre) and the dialog windows (Settings with
 * Pitcrew's pages beside Appearance, About, Updates, What's new, the confirm and the stop-process decisions). Every shot waits
 * for loaded content: no skeleton, no splash, fonts and images decoded, the layout still.
 */
const OUT = process.env['SHOTS_OUT'] ?? '../docs/screenshots';
const NOW = new Date('2026-10-22T14:40:00');

for (const theme of ['dark', 'light'] as const) {
  test.describe(`${theme} shell surfaces`, () => {
    test.use({ colorScheme: theme, timezoneId: 'UTC', locale: 'en-US', viewport: { width: 1440, height: 900 } });
    test.beforeEach(async ({ page }) => {
      await page.clock.setFixedTime(NOW);
      await page.addInitScript((t) => localStorage.setItem('pitcrew.theme', t), theme);
      await page.emulateMedia({ reducedMotion: 'reduce' });
    });

    const shoot = async (page: import('@playwright/test').Page, name: string) => {
      await loaded(page);
      await page.screenshot({ path: `${OUT}/${theme}/shell-${name}.png` });
    };

    test('jobs list with a running update and Cancel', async ({ page, app }) => {
      await page.goto('/?simulate=0#/updates');
      const row = page.getByRole('row', { name: /ui-tests/ });
      await row.getByRole('button', { name: 'Update ui-tests to 5.3.0' }).click();
      await app.statusBar.jobs.click();
      await expect(app.jobs.list).toContainText('Updating');
      await shoot(page, 'jobs');
    });

    test('a service that is down, with its Restart', async ({ page, app }) => {
      await page.goto('/?simulate=0&mock=notInstalled#/');
      await expect(app.statusBar.chip).toContainText(/Pitcrew service/);
      await app.statusBar.chip.click();
      await expect(app.notifications.services).toContainText('cannot reach its background service');
      await shoot(page, 'health-down');
    });

    test('the notification centre', async ({ page, app }) => {
      await page.goto('/?simulate=0&mock=notInstalled#/');
      await expect(app.statusBar.chip).toContainText(/Pitcrew service/);
      await app.notifications.openCentre();
      await shoot(page, 'notification-centre');
    });

    // Dialog windows are shot at the size their definition declares (a fitted one at its width and the content's height).
    const windows: [string, string, { width: number; height: number }, unknown][] = [
      ['settings', 'pd.settings', { width: 920, height: 680 }, {}],
      ['updates', 'pd.updates', { width: 560, height: 480 }, {}],
      ['about', 'pd.about', { width: 680, height: 560 }, {}],
      ['whats-new', 'pd.whats-new', { width: 680, height: 560 }, {}],
      ['confirm', 'pc.confirm', { width: 440, height: 200 }, { title: 'Delete the runner folder', body: 'The folder of "build-win" and everything in it is deleted. This cannot be undone.', confirm: 'Delete folder', danger: true }],
      ['stop-process', 'pc.stop-process', { width: 460, height: 260 }, { name: 'node.exe', pid: 4312, isRunner: false, jobRunning: true }],
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
