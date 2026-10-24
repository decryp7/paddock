import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// The "Notifications" settings page (plan X8) in the shell demo: the section Rust wrote into its fixture, over the demo's
// in-memory engine. The OS side (policy, limits, quiet hours, digest, clicks) is tested once in paddock-health.

test('the Notifications page shows the OS notification choices and takes a change', async ({ app, page }) => {
  await app.settings.open({ page: 'pd.notifications' });
  await expect(app.settings.nav.locator('[aria-current="page"]')).toHaveText('Notifications');
  await expect(app.settings.field('level').getByRole('combobox', { name: 'Show notifications for' })).toContainText('Problems and things that need me');
  await app.settings.choose('level', 'Nothing');
  await expect(app.settings.saved('Show notifications for saved.')).toBeVisible();
  await expect(app.settings.field('quietStart')).toContainText('Quiet from');
  await expect(app.settings.field('cooldownSecs')).toContainText('seconds');
  await app.settings.field('quietStart').getByRole('textbox').fill('25:00');
  await app.settings.field('quietStart').getByRole('textbox').press('Enter');
  await expect(app.settings.field('quietStart').getByRole('alert')).toBeVisible();
  await expectNoHorizontalScroll(page);
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});
