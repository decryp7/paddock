import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > Notification channels (plan X17) in the shell demo: the page over the demo's in-memory engine, which plays
// `paddock-channels` (the same validation, secrets that never leave the engine, a test that ends by an event).
// `?channels=empty|many|error` picks the channels it starts with, `?channels-test=succeed|rejected|unreachable|hang` how a
// test ends. Chromium only in branches (rule 3). Only the test kit.

test.describe('notification channels (plan X17)', () => {
  test('add a webhook, set its token in the dialog, test it, edit it and remove it', async ({ app, page, pageErrors }) => {
    // From the main window: the Settings window and the secret dialog both open in the page (a dialog window of the browser
    // build cannot open another one; on the desktop it is an OS window owned by the Settings window).
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    await app.settings.nav.getByRole('button', { name: 'Notification channels' }).click();
    await expect(page.locator('tbody tr')).toHaveCount(2);
    expect(await axeViolations(page)).toEqual([]);
    await page.getByTestId('channel-add').click();
    await expect(page.getByTestId('channel-name')).toBeFocused();

    await page.getByTestId('channel-name').fill('Build Hook');
    await page.getByTestId('channel-url').fill('https://hooks.example.com/services/abc');
    await page.getByTestId('channel-event-diskLow').getByRole('checkbox').check();
    await page.getByTestId('channel-event-jobFailed').getByRole('checkbox').check();
    await page.getByTestId('channel-auth').getByRole('checkbox').check();
    await expect(page.getByTestId('channel-secret-state')).toHaveText('No secret yet');

    // The token is typed in the dialog and goes to the engine; the page only learns that one is stored.
    await page.getByTestId('channel-secret-set').click();
    const dialog = page.getByRole('dialog', { name: 'Token for Build Hook' });
    await expect(dialog).toBeVisible();
    const input = dialog.getByTestId('channel-secret-input');
    await expect(input).toHaveAttribute('type', 'password');
    await expect(dialog.getByTestId('dialog-action-save')).toBeDisabled();
    await input.fill('xoxb-secret-token');
    // The Show button is exactly as tall as the input.
    const [inputBox, toggleBox] = await Promise.all([input.boundingBox(), dialog.getByTestId('channel-secret-toggle').boundingBox()]);
    expect(toggleBox!.height).toBe(inputBox!.height);
    expect(Math.abs(toggleBox!.y - inputBox!.y)).toBeLessThanOrEqual(0.5);
    await dialog.getByTestId('channel-secret-toggle').click();
    await expect(input).toHaveAttribute('type', 'text');
    await input.press('Enter');
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId('channel-secret-state')).toHaveText('Secret stored');
    await expect(page.locator('body')).not.toContainText('xoxb-secret-token');

    // A test of the draft: Still working, then Delivered next to the button.
    await page.getByTestId('channel-send-test').click();
    await expect(page.getByTestId('channel-test-result-Build-Hook')).toContainText('Still working');
    await expect(page.getByTestId('channel-test-result-Build-Hook')).toContainText('Delivered');

    await page.getByTestId('channel-save').click();
    const row = page.getByTestId('channel-row-Build-Hook');
    await expect(row).toContainText('Build Hook');
    await expect(row).toContainText('diskLow, jobFailed');
    await expect(row.getByText('On', { exact: true })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);

    await row.hover();
    await row.getByRole('button', { name: 'Edit Build Hook' }).click();
    await page.getByTestId('channel-name').fill('Ops Hook');
    await page.getByTestId('channel-save').click();
    await expect(page.getByTestId('channel-row-Build-Hook')).toContainText('Ops Hook');

    // Remove asks in a dialog of its own: Keep is the default and Escape keeps the channel.
    await page.getByTestId('channel-row-Build-Hook').getByRole('button', { name: 'Remove Ops Hook' }).click();
    const confirm = page.getByRole('alertdialog', { name: 'Remove the channel “Ops Hook”?' });
    await expect(confirm).toContainText('Its stored secret is deleted too.');
    await expect(confirm.getByRole('button', { name: 'Keep' })).toBeFocused();
    expect(await axeViolations(page)).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(confirm).toHaveCount(0);
    await expect(page.getByTestId('channel-row-Build-Hook')).toBeVisible();
    await page.getByTestId('channel-row-Build-Hook').getByRole('button', { name: 'Remove Ops Hook' }).click();
    await page.getByRole('alertdialog', { name: 'Remove the channel “Ops Hook”?' }).getByRole('button', { name: 'Remove channel', exact: true }).click();
    await expect(page.locator('tbody tr')).toHaveCount(2);
    await expect(page.getByTestId('channel-row-Build-Hook')).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('a failed test says why, in the engine’s words, next to the row', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels-test=rejected');
    const row = page.getByTestId('channel-row-slack');
    await row.getByRole('button', { name: 'Send a test to Team Slack' }).click();
    const result = page.getByTestId('channel-test-result-slack');
    await expect(result).toContainText('Still working');
    await expect(result).toContainText('HTTP 401');
    await expect(result).toHaveAttribute('role', 'alert');
    expect(await axeViolations(page)).toEqual([]);
  });

  test('a test shows its attempt and phase while it retries, then Delivered after 2 attempts', async ({ app, page }) => {
    // The demo engine's steps wait for the test (`?channels-step=manual`), so each phase is seen for as long as the test
    // wants: a poll can miss a phase that lasts as long as a 400 ms timer.
    await app.settings.open({ page: 'pd.channels' }, '?channels-test=retry&channels-step=manual');
    const step = () => page.evaluate(() => (window as unknown as { __sdChannelsStep: () => void }).__sdChannelsStep());
    await page.getByRole('button', { name: 'Send a test to Team Slack' }).click();
    const progress = page.getByTestId('channel-test-progress-slack');
    await step();
    await expect(progress).toHaveText('Attempt 1 of 3 · sending…');
    await step();
    await expect(progress).toHaveText('Attempt 1 of 3 · waiting 4 s before the next try');
    await step();
    await expect(progress).toHaveText('Attempt 2 of 3 · sending…');
    await step();
    await expect(page.getByTestId('channel-test-result-slack')).toHaveText('Delivered after 2 attempts');
  });

  test('a test that hangs keeps saying Still working', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels-test=hang');
    await page.getByRole('button', { name: 'Send a test to Team Slack' }).click();
    await expect(page.getByTestId('channel-test-result-slack')).toContainText('Still working');
    await expect(page.getByTestId('channel-test-progress-slack')).toHaveText('Attempt 1 of 3 · sending…');
    await page.waitForTimeout(1500);
    await expect(page.getByTestId('channel-test-result-slack')).toContainText('Still working');
  });

  test('a load error is inline with Retry', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=error');
    const error = page.getByTestId('channels-error');
    await expect(error).toHaveAttribute('role', 'alert');
    await error.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByTestId('channels-empty')).toBeVisible();
  });

  test('the engine’s refusal of a save is shown as it is', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=empty');
    await page.getByTestId('channel-add-empty').click();
    await page.getByTestId('channel-name').fill('Plain http');
    await page.getByTestId('channel-url').fill('http://example.com/hook');
    await page.getByTestId('channel-save').click();
    await expect(page.getByTestId('channel-save-error')).toContainText('must start with https://');
    await expect(page.getByTestId('channel-editor')).toBeVisible();
  });

  test('many channels: Add is disabled at the limit with a reason, and 320 px has no horizontal scroll', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=many');
    await expect(page.locator('tbody tr')).toHaveCount(8);
    await expect(page.getByTestId('channel-add')).toHaveAttribute('aria-disabled', 'true');
    await expect(page.getByTestId('channels-limit')).toContainText('Remove one');
    await page.getByTestId('channel-row-slack').getByRole('button', { name: 'Edit Team Slack' }).click();
    expect(await axeViolations(page)).toEqual([]);
    await page.setViewportSize({ width: 320, height: 800 });
    await expectNoHorizontalScroll(page);
    await expect(page.getByTestId('channel-row-slack')).toBeVisible();
    await page.getByTestId('channel-cancel').click();
    await expectNoHorizontalScroll(page);
  });

  test('the demo declares events: grouped checkboxes, unknown names kept as chips, a home-network switch', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=many');
    await page.getByRole('button', { name: 'Edit Team Slack' }).click();
    const picker = page.getByTestId('channel-events-picker');
    await expect(picker.getByRole('group', { name: 'Storage' })).toBeVisible();
    await expect(picker.getByRole('group', { name: 'Jobs' })).toBeVisible();
    await expect(page.getByTestId('channel-event-diskLow').getByRole('checkbox')).toBeChecked();
    await expect(page.getByTestId('channel-event-jobDone').getByRole('checkbox')).not.toBeChecked();
    await expect(page.getByTestId('channel-events-other').getByRole('listitem')).toHaveText(['oldAlarm']);
    await page.getByTestId('channel-event-jobDone').getByRole('checkbox').check();
    await expect(page.getByTestId('channel-allow-lan').getByRole('checkbox')).not.toBeChecked();
    expect(await axeViolations(page)).toEqual([]);
    await page.getByTestId('channel-save').click();
    await expect(page.getByTestId('channel-editor')).toHaveCount(0);
    await expect(page.getByTestId('channel-row-slack')).toContainText('diskLow, jobFailed and 2 more');
  });

  test('an app with no declared events keeps the free-text field', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels-events=none');
    await page.getByRole('button', { name: 'Edit Team Slack' }).click();
    await expect(page.getByTestId('channel-events-picker')).toHaveCount(0);
    await expect(page.getByTestId('channel-events')).toHaveValue('diskLow, jobFailed, oldAlarm');
    expect(await axeViolations(page)).toEqual([]);
  });

  test('a private address needs the home-network switch, also for a mail server', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=empty');
    await page.getByTestId('channel-add-empty').click();
    await page.getByTestId('channel-name').fill('NAS');
    await page.getByTestId('channel-url').fill('http://192.168.1.20:8080/hook');
    await page.getByTestId('channel-save').click();
    await expect(page.getByTestId('channel-save-error')).toContainText('home network');
    await page.getByTestId('channel-allow-lan').getByRole('checkbox').check();
    await page.getByTestId('channel-save').click();
    await expect(page.getByTestId('channel-editor')).toHaveCount(0);
    await expect(page.getByTestId('channel-row-NAS')).toContainText('192.168.1.20');
    await page.getByRole('button', { name: 'Edit NAS' }).click();
    await expect(page.getByTestId('channel-allow-lan').getByRole('checkbox')).toBeChecked();
    await page.locator('#pd-ch-type').click();
    await page.getByRole('option', { name: 'Email' }).click();
    await expect(page.getByTestId('channel-allow-lan')).toHaveCount(1);
  });

  test('works by keyboard alone: Tab to Edit, Escape cancels, Enter saves', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '');
    const edit = page.getByRole('button', { name: 'Edit Phone' });
    await edit.focus();
    await expect(edit).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('channel-name')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('channel-editor')).toHaveCount(0);
    await edit.focus();
    await page.keyboard.press('Enter');
    await page.getByTestId('channel-topic').fill('keyboard_topic');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('channel-editor')).toHaveCount(0);
    await expect(page.getByTestId('channel-row-phone')).toContainText('keyboard_topic');
  });
});
