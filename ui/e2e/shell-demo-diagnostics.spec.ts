import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Report a problem (plan X2) in the shell demo: the dialog over the demo's in-memory engine, which plays `paddock-diag`
// (the same preview shape, outcomes and refusals; `?report=dismiss|fail|hang` picks what Save does). Nothing is written
// anywhere in the browser build, and the dialog never shows a path field: the engine asks the user where to save.
// Chromium only in branches (rule 3). Only the test kit (@decryp7/paddock-e2e, plan S16).

test('Help ▸ Report a problem… shows every file with its size, follows the description, and saves where the engine says', async ({ app, page, pageErrors }) => {
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  await (await app.menus.item('Help', 'Report a problem…')).click();
  const dialog = app.dialogs.inPage('Report a problem');
  await expect(dialog).toHaveAttribute('aria-modal', 'false');
  await expect(dialog).toContainText('Nothing is sent anywhere.');
  const rows = dialog.locator('[data-testid="diag-files"] tbody tr');
  await expect(rows).toHaveCount(6);
  await expect(rows.first()).toContainText('manifest.json');
  await expect(rows.nth(4)).toContainText('logs/shell-demo.2026-10-24.log');
  await expect(rows.nth(4)).toContainText('182 KB');
  await expect(dialog.getByTestId('diag-removed')).toContainText('Removed from the files:');
  // No way to type a path: where the zip goes is the engine's native dialog.
  await expect(dialog.getByRole('textbox')).toHaveCount(1);
  await expect(dialog.getByRole('textbox')).toHaveAttribute('id', 'pd-diag-description');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);

  await dialog.getByRole('textbox').fill('The timeline stays empty after a reload');
  await dialog.getByRole('textbox').blur();
  await expect(rows.nth(1)).toContainText('Your description (39 characters)');
  await dialog.getByRole('button', { name: 'Save report…' }).click();
  await expect(dialog.getByTestId('diag-saved')).toContainText('The report is saved.');
  await expect(dialog.getByTestId('diag-saved-path')).toHaveAttribute('title', /demo-diagnostics-.*\.zip$/);
  await expect(dialog.getByRole('button', { name: 'Save report…' })).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(dialog).toHaveCount(0);
  expect(pageErrors).toEqual([]);
});

test('a closed save dialog and a failed write say what happened and let the user try again', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.report-problem', {}, { title: 'Report a problem', query: '?report=dismiss' });
  await page.getByRole('button', { name: 'Save report…' }).click();
  await expect(page.getByTestId('diag-dismissed')).toContainText('Nothing was saved.');
  await expect(page.getByRole('button', { name: 'Save report…' })).toBeEnabled();

  await app.dialogs.openWindow('pd.report-problem', {}, { title: 'Report a problem', query: '?report=fail' });
  await page.getByRole('button', { name: 'Save report…' }).click();
  const alert = page.getByTestId('diag-save-error');
  await expect(alert).toHaveAttribute('role', 'alert');
  await expect(alert).toContainText('No space left on device. Choose another folder or free some space.');
  await expect(page.getByRole('button', { name: 'Save report…' })).toBeEnabled();
});

test('Stop saving cancels the save and nothing is written', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.report-problem', {}, { title: 'Report a problem', query: '?report=hang' });
  await page.getByRole('button', { name: 'Save report…' }).click();
  const saving = page.getByTestId('diag-saving');
  await expect(saving).toContainText('Saving the report…');
  await expect(page.getByRole('textbox')).toBeDisabled();
  await saving.getByRole('button', { name: 'Stop saving' }).click();
  await expect(page.getByTestId('diag-cancelled')).toContainText('Nothing was written.');
  await expect(page.getByRole('button', { name: 'Save report…' })).toBeEnabled();
});

test('the dialog lays out at a narrow width without a horizontal scroll', async ({ app, page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await app.dialogs.openWindow('pd.report-problem', {}, { title: 'Report a problem' });
  await expect(page.getByTestId('diag-files')).toBeVisible();
  await expectNoHorizontalScroll(page);
});

test('the Logging settings page shows the level and takes a change', async ({ app, page }) => {
  await app.settings.open({ page: 'pd.logging' });
  await expect(app.settings.nav.locator('[aria-current="page"]')).toHaveText('Logging');
  await expect(app.settings.field('level').getByRole('combobox', { name: 'Detail' })).toContainText('Normal');
  await app.settings.choose('level', 'Detailed (for support cases)');
  await expect(app.settings.saved('Detail saved.')).toBeVisible();
  await expect(app.settings.field('maxFileMb')).toContainText('Needs restart');
  await expect(app.settings.field('keepFiles')).toContainText('days');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});
