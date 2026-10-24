import { axeViolations, expect, expectNoHorizontalScroll, test, THEMES } from '@decryp7/paddock-e2e';

// Settings (plan S10) in the shell demo: the Settings window as a modeless tool dialog over the demo's in-memory engine,
// which serves the sections Rust wrote into the fixtures (every field kind). Chromium only in branches (rule 3). Only the
// test kit (@decryp7/paddock-e2e, plan S16).

test.describe('settings (plan S10)', () => {
  test('the Settings window opens in the page from its command; a change reaches every window', async ({ app, page, pageErrors }) => {
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    const win = app.settings.window;
    await expect(win).toHaveAttribute('aria-modal', 'false');
    await expect(app.settings.nav.getByRole('button')).toHaveText(['Appearance', 'Language', 'Background service', 'Set-up', 'General', 'Storage', 'Notifications', 'Logging']);
    // The Appearance section is the shell's; its change applies to the main window at once.
    await app.settings.choose('theme', 'Dark', win);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(app.settings.saved('Theme saved.', win)).toBeVisible();
    await app.settings.field('theme', win).getByRole('button', { name: 'Reset to default: Theme' }).click();
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
    // The keyboard shortcut opens the same (single) window.
    await page.getByRole('button', { name: 'Say hello' }).focus();
    await page.keyboard.press('ControlOrMeta+Comma');
    await expect(page.getByRole('dialog', { name: 'Settings' })).toHaveCount(1);
    // The window's own Close (the page's dialog frame has one too).
    await win.getByText('Close', { exact: true }).click();
    await expect(win).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('every field kind is rendered from the schema and its changes are applied', async ({ app, page }) => {
    await app.settings.open({ page: 'sd.general' });
    await expect(app.settings.nav.locator('[aria-current="page"]')).toHaveText('General');
    await expect(app.settings.field('notify').getByRole('switch', { name: 'Show a notification' })).toBeChecked();
    await expect(app.settings.select('style')).toHaveText('Casual');
    await expect(app.settings.field('greeting').getByRole('textbox', { name: 'Greeting' })).toHaveValue('Hello');
    await expect(app.settings.field('repeat').getByRole('spinbutton', { name: 'Repeat' })).toHaveValue('1');
    await expect(app.settings.field('volume')).toContainText('Needs restart');

    await app.settings.field('notify').getByRole('switch').uncheck();
    await expect(app.settings.saved('Show a notification saved.')).toBeVisible();
    await app.settings.field('volume').getByRole('spinbutton').fill('0.8');
    await app.settings.field('volume').getByRole('spinbutton').press('Enter');
    await expect(page.getByTestId('settings-restart')).toContainText('1 change takes effect after the app restarts (Volume).');

    await app.settings.nav.getByRole('button', { name: 'Storage' }).click();
    await expect(app.settings.field('folder').getByRole('textbox', { name: 'Data folder' })).toHaveValue('');
    await expect(app.settings.field('keep')).toContainText('min');
    await expect(app.settings.field('cache')).toContainText('MB');
    await expect(app.settings.field('ignore')).toHaveCount(0);
    await page.getByText('Show advanced').first().click();
    await expect(app.settings.field('ignore').getByRole('textbox', { name: 'Ignored names' })).toHaveValue('*.tmp');
    await app.settings.field('keep').getByRole('spinbutton').fill('90');
    await app.settings.field('keep').getByRole('spinbutton').press('Tab');
    await expect(app.settings.saved('Keep logs for saved.')).toBeVisible();
    await app.settings.field('folder').getByRole('textbox').fill('relative/path');
    await app.settings.field('folder').getByRole('textbox').press('Enter');
    await expect(app.settings.field('folder').getByRole('alert')).toHaveText('Must be an absolute path (or empty).');
  });

  test('a refused edit stays unsaved, is marked, and is asked about before the window closes', async ({ app, page }) => {
    await app.settings.open({ page: 'sd.general' });
    const greeting = app.settings.field('greeting').getByRole('textbox', { name: 'Greeting' });
    await greeting.fill('Hi there!');
    await greeting.press('Enter');
    await expect(app.settings.field('greeting').getByRole('alert')).toHaveText('Does not have the expected format ([A-Za-z ,]+).');
    await expect(greeting).toHaveAttribute('aria-invalid', 'true');
    await expect(app.settings.nav.getByRole('button', { name: /General/ }).getByRole('img', { name: 'unsaved changes' })).toBeVisible();
    // Moving between pages keeps the edit.
    await app.settings.nav.getByRole('button', { name: 'Storage' }).click();
    await app.settings.nav.getByRole('button', { name: /General/ }).click();
    await expect(greeting).toHaveValue('Hi there!');
    // Escape asks; Keep editing keeps the window; Discard closes it.
    await page.keyboard.press('Escape');
    const ask = page.getByRole('alertdialog');
    await expect(ask).toContainText('Some changes are not saved: General.');
    expect(await app.dialogs.result()).toBeNull();
    await ask.getByRole('button', { name: 'Keep editing' }).click();
    await expect(ask).toHaveCount(0);
    await page.getByRole('button', { name: 'Close', exact: true }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Discard changes' }).click();
    await app.dialogs.expectResult('{"result":null}');
  });

  test('search finds settings however they are written, and the window is keyboard operable', async ({ app, page }) => {
    await app.settings.open();
    const search = app.settings.search;
    await search.fill('DataFodler');
    await expect(page.locator('pd-setting-field')).toHaveCount(1);
    await expect(app.settings.field('folder')).toBeVisible();
    await search.fill('ignored names');
    await expect(app.settings.field('ignore')).toBeVisible();
    // Escape clears a filled search without closing the window.
    await search.press('Escape');
    await expect(search).toHaveValue('');
    await expect(page.getByTestId('settings-content').locator('pd-settings-section')).toHaveCount(1);
    // The pages by arrow keys.
    await app.settings.nav.getByRole('button', { name: 'Appearance' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(app.settings.nav.getByRole('button', { name: 'Language' })).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(app.settings.nav.getByRole('button', { name: 'Background service' })).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(app.settings.nav.getByRole('button', { name: 'Set-up' })).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(app.settings.nav.getByRole('button', { name: 'General' })).toBeFocused();
    await page.keyboard.press('End');
    // The last page is the shell's Logging (plan X2, in the advanced group).
    await expect(app.settings.nav.getByRole('button', { name: 'Logging' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(app.settings.nav.locator('[aria-current="page"]')).toHaveText('Logging');
    // Nothing unsaved: Escape closes.
    await page.keyboard.press('Escape');
    await app.dialogs.expectResult('{"result":null}');
  });

  test('a damaged settings file gives defaults, one notification and a note in the Settings window', async ({ app, page }) => {
    await page.goto('/?settings=damaged');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const centre = await app.notifications.openCentre();
    const notice = app.notifications.notice('settings:file');
    await expect(notice).toHaveCount(1);
    await expect(notice).toContainText('Settings were reset to defaults');
    await expect(notice).not.toContainText('×');
    // Its action opens Settings, which says what happened.
    await notice.getByRole('button', { name: 'Open Settings…' }).click();
    const win = app.settings.window;
    await expect(win.getByTestId('settings-problem')).toContainText('Defaults are in use; the damaged file was kept as settings.json.corrupt');
    await expect(app.settings.select('theme', win)).toHaveText('Same as the system');
    // The centre's popover stays open beside the window; closing it leaves the window.
    if (await centre.isVisible()) await app.statusBar.bell.click();
    await win.getByText('Close', { exact: true }).click();
    await expect(win).toHaveCount(0);
  });

  for (const colorScheme of THEMES) {
    test(`the Settings window has no axe violations (${colorScheme})`, async ({ app, page }) => {
      await page.emulateMedia({ colorScheme });
      await app.settings.open({ page: 'sd.general' }, '?settings=damaged');
      await expect(app.settings.field('greeting')).toBeVisible();
      await app.settings.field('greeting').getByRole('textbox').fill('Hi!');
      await app.settings.field('greeting').getByRole('textbox').press('Enter');
      await expect(app.settings.field('greeting').getByRole('alert')).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      await app.settings.search.fill('size');
      await expect(app.settings.field('cache')).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
    });
  }

  test('no horizontal scroll in the Settings window at 320 px (rule 18)', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await app.settings.open({ page: 'sd.storage' });
    await expect(app.settings.field('folder')).toBeVisible();
    await expectNoHorizontalScroll(page);
  });

  test('the open select list is as wide as its trigger and starts at its left edge (review 2, N-P2-1)', async ({ app, page }) => {
    await app.settings.open({ page: 'sd.general' });
    const trigger = app.settings.select('style');
    await trigger.click();
    const list = page.locator('.pd-list');
    await expect(list).toBeVisible();
    const [t, l] = [(await trigger.boundingBox())!, (await list.boundingBox())!];
    expect(Math.abs(l.x - t.x)).toBeLessThanOrEqual(1);
    expect(l.width).toBeGreaterThanOrEqual(t.width - 1);
    expect(l.width).toBeLessThanOrEqual(t.width + 1);
  });
});
