import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// The first-run set-up checklist (plan X19) in the shell demo: the Set-up window and Settings > Set-up over the demo's
// in-memory engine, which plays `paddock_shell::setup` (the same states, the working-then-outcome events, the refusals).
// `?setup=fresh` starts as a first run, `?setup-outcome=fail|hang` decides how a step ends. Chromium only in branches
// (rule 3). Only the test kit.

test.describe('set-up checklist (plan X19)', () => {
  test('the window lists the steps, a step walks to done and leaves nothing to press', async ({ app, page, pageErrors }) => {
    await app.dialogs.openWindow('pd.setup', {}, { title: 'Set-up', query: '?setup=fresh' });
    await app.dialogs.loaded();
    await expect(page.getByTestId('setup-list').getByRole('heading')).toHaveText(['Background service', 'Start at login', 'Notifications', 'Connect to Pitwall']);
    await expect(page.getByTestId('setup-status-service')).toContainText('To do');
    await expect(page.getByRole('button', { name: 'Install' })).toHaveClass(/pd-setup__btn--primary/);
    expect(await axeViolations(page)).toEqual([]);
    await expectNoHorizontalScroll(page);

    await page.getByTestId('setup-act-service').click();
    await expect(page.getByTestId('setup-working-service')).toHaveAccessibleName('Working…');
    await expect(page.getByTestId('setup-status-service')).toContainText('Done');
    await expect(page.getByTestId('setup-act-service')).toHaveCount(0);
    await expect(page.getByTestId('setup-working-service')).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('a step shows "Still working" for as long as it runs, and nothing gives up on it', async ({ app, page }) => {
    await app.dialogs.openWindow('pd.setup', {}, { title: 'Set-up', query: '?setup=fresh&setup-outcome=hang' });
    await page.getByTestId('setup-act-login').click();
    await expect(page.getByTestId('setup-still-working')).toContainText('Still working');
    await expect(page.getByTestId('setup-act-login')).toBeDisabled();
    await expect(page.getByTestId('setup-skip-login')).toBeDisabled();
    await page.waitForTimeout(1500);
    await expect(page.getByTestId('setup-still-working')).toBeVisible();
  });

  test('a failed step says why and its Repair is the primary button', async ({ app, page }) => {
    await app.dialogs.openWindow('pd.setup', {}, { title: 'Set-up', query: '?setup=fresh&setup-outcome=fail' });
    await page.getByTestId('setup-act-notifications').click();
    await expect(page.getByTestId('setup-status-notifications')).toContainText('Needs repair');
    await expect(page.getByTestId('setup-message-notifications')).toContainText('Close the other copy of the app');
    await expect(page.getByTestId('setup-act-notifications')).toHaveCount(0);
    await expect(page.getByTestId('setup-repair-notifications')).toHaveClass(/pd-setup__btn--primary/);
    await expect(page.getByTestId('setup-repair-notifications')).toBeEnabled();
  });

  test('every step can be skipped and taken up again, and Pitwall is optional', async ({ app, page }) => {
    await app.dialogs.openWindow('pd.setup', {}, { title: 'Set-up', query: '?setup=fresh' });
    await expect(page.getByTestId('setup-detail-pitwall')).toContainText('not installed');
    await page.getByTestId('setup-skip-pitwall').click();
    await expect(page.getByTestId('setup-status-pitwall')).toContainText('Skipped');
    await expect(page.getByTestId('setup-skip-pitwall')).toHaveText('Take it up again');
    await page.getByTestId('setup-skip-pitwall').click();
    await expect(page.getByTestId('setup-status-pitwall')).toContainText('To do');
  });

  test('the same checklist is in Settings > Set-up', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.setup' }, '?setup=fresh');
    await expect(page.getByTestId('setup-page')).toContainText('Set-up');
    await expect(page.getByTestId('setup-item-service')).toBeVisible();
    await page.getByTestId('setup-act-login').click();
    await expect(page.getByTestId('setup-status-login')).toContainText('Done');
    expect(await axeViolations(page)).toEqual([]);
  });

  test('the checklist opens by itself on the first start and not after', async ({ page, pageErrors }) => {
    await page.goto('/?setup=fresh');
    const dialog = page.getByRole('dialog', { name: 'Set-up' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute('aria-modal', 'false');
    await dialog.getByTestId('dialog-action-dismiss').click();
    await expect(dialog).toHaveCount(0);
    await page.reload();
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await expect(page.getByRole('dialog', { name: 'Set-up' })).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('without ?setup=fresh the demo does not open it unasked, and Help > Set-up… does', async ({ app, page }) => {
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await expect(page.getByRole('dialog', { name: 'Set-up' })).toHaveCount(0);
    const help = await app.menus.open('Help');
    await help.getByRole('menuitem', { name: 'Set-up…' }).click();
    await expect(page.getByRole('dialog', { name: 'Set-up' })).toBeVisible();
  });
});
