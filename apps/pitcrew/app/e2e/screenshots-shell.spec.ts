import { captureScreen, expect, test } from '@decryp7/paddock-e2e';

/**
 * The shell surfaces of Pitcrew for review (`SCREENSHOTS=1 SHOTS_OUT=<folder> npx playwright test --project=screenshots`): the
 * whole 1440x900 window at 2x, in both themes, never cropped. The shell's parts (the Jobs list with the disk clean-up, a service chip that is down with its Restart, the notification centre) and the dialog windows (Settings with
 * Pitcrew's pages beside Appearance, About, Updates, What's new, the confirm and the stop-process decisions). Every image is
 * taken with `captureScreen` (loaded content, nothing cut, nothing covered).
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

    const shoot = async (page: import('@playwright/test').Page, name: string, fullPage: 'viewport' | 'main' = 'viewport') => {
      await captureScreen(page, `${OUT}/${theme}/shell-${name}.png`, { fullPage });
    };

    test('jobs list with the disk clean-up', async ({ page, app }) => {
      await page.goto('/?simulate=0#/runners/build-win?tab=disk');
      await page.getByRole('button', { name: /Clean up now|Clean now/ }).first().click();
      await app.statusBar.jobs.click();
      await expect(app.jobs.list).toContainText('Cleaning up the disk');
      // A page with a long table: taken whole, with the Jobs list open over it.
      await shoot(page, 'jobs', 'main');
    });

    test('a service that is down, with its Restart', async ({ page, app }) => {
      await page.goto('/?simulate=0&mock=notInstalled#/');
      await expect(app.statusBar.chip).toContainText(/Pitcrew service/);
      await app.statusBar.chip.click();
      await expect(app.notifications.services).toContainText('is not installed');
      await shoot(page, 'health-down');
    });

    test('the notification centre', async ({ page, app }) => {
      await page.goto('/?simulate=0&mock=notInstalled#/');
      await expect(app.statusBar.chip).toContainText(/Pitcrew service/);
      await app.notifications.openCentre();
      await shoot(page, 'notification-centre');
    });

    test('config diff window', async ({ page, app }) => {
      await page.setViewportSize({ width: 720, height: 560 });
      const line = (kind: string, text: string, oldNo: number | null, newNo: number | null) => ({ kind, text, oldNo, newNo });
      const preview = {
        text: '',
        changed: true,
        restartNeeded: true,
        diff: {
          hunks: [
            { lines: [line('same', 'runner:', 1, 1), line('removed', '  capacity: 1', 2, null), line('added', '  capacity: 4', null, 2), line('removed', '  timeout: 3h', 3, null), line('added', '  timeout: 2h', null, 3), line('same', '  fetch_interval: 2s', 4, 4)] },
          ],
          added: 2,
          removed: 2,
          truncated: false,
        },
        issues: [],
        changedKeys: ['runner.capacity', 'runner.timeout'],
        labels: [],
      };
      await app.dialogs.openWindow('pc.config-diff', { preview, maxBackups: 5 });
      await app.dialogs.loaded();
      await shoot(page, 'config-diff');
    });

    test('register window, last step', async ({ page, app }) => {
      await page.setViewportSize({ width: 660, height: 640 });
      await app.dialogs.openWindow('pc.register', {});
      await app.dialogs.loaded();
      const dialog = page.locator('pd-dialog-shell');
      await dialog.getByLabel('Server URL').fill('https://git.example.com');
      await dialog.getByLabel('Registration token').fill('demo-registration-token');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByLabel('Name in Pitcrew').fill('nightly');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByText('Run as the service account').click();
      await shoot(page, 'register-wizard');
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
