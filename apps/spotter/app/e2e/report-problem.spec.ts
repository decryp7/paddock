import { axeViolations, expect, test } from '@decryp7/paddock-e2e';

// Report a problem in Spotter's browser build: the shell's dialog over the in-memory engine that plays `paddock-diag` (the
// same preview shape). The real engine's redaction of feed addresses is tested in Rust (src-tauri/src/diagnostics.rs).

test.describe('Report a problem', () => {
  test('Help > Report a problem… lists Spotter’s files with their sizes and saves where the engine says', async ({ app, page, pageErrors }) => {
    await page.goto('/');
    await (await app.menus.item('Help', 'Report a problem…')).click();
    const dialog = app.dialogs.inPage('Report a problem');
    await expect(dialog).toContainText('Nothing is sent anywhere.');
    const rows = dialog.locator('[data-testid="diag-files"] tbody tr');
    await expect(rows.filter({ hasText: 'system-info.json' })).toHaveCount(1);
    await expect(rows.filter({ hasText: 'logs/spotter.' })).toContainText('addresses keep only their host');
    expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
    await dialog.getByRole('button', { name: 'Save report…' }).click();
    await expect(dialog.getByTestId('diag-saved')).toContainText('The report is saved.');
    await dialog.getByRole('button', { name: 'Done' }).click();
    await expect(dialog).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('Settings has the shell’s Logging page', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.logging' });
    await expect(app.settings.nav.locator('[aria-current="page"]')).toHaveText('Logging');
    await expect(app.settings.field('level').getByRole('combobox', { name: 'Detail' })).toContainText('Normal');
    expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
  });
});
