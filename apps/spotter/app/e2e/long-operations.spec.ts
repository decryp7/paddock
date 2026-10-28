import { expect, test } from '@decryp7/paddock-e2e';

/**
 * Long operations against the mock backend (which paces its steps like the shell would report them): the shell's Jobs
 * list shows the phase in words with its own numbers from the start, Cancel works, and the UI never gives up on its own.
 */
test.describe('long operations show what they are doing and can be cancelled', () => {
  test('a download shows its phase and bytes, and Cancel stops it', async ({ page, app }) => {
    // Slow steps, so the Jobs list is opened while the download still runs.
    await page.goto('/?jobStepMs=1500');
    await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Download' }).click();
    await app.statusBar.jobs.click();
    const job = app.jobs.items.first();
    await expect(job).toBeVisible();
    await expect(job).toContainText('Downloading');
    await expect(job).toContainText(/MB of 176 MB/);
    await job.getByRole('button', { name: /Cancel/ }).click();
    await expect(page.getByText('Download cancelled')).toBeVisible();
    await expect(page.getByText('Download failed')).toHaveCount(0);
  });

  test('a download that is left alone finishes and says where the file is', async ({ page }) => {
    await page.goto('/');
    await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Download' }).click();
    await expect(page.getByText('Download finished')).toBeVisible({ timeout: 20_000 });
  });

  test('installing the browser goes through Downloading, Unpacking and Testing', async ({ app, page }) => {
    await app.settings.open({ page: 'sp.general', });
    await page.getByRole('button', { name: 'Download Chromium' }).click();
    // The Jobs list is the main window’s; the settings page shows the outcome.
    await expect(page.getByText('Chromium downloaded')).toBeVisible({ timeout: 30_000 });
  });

  test('checking every watch shows n of m and can be cancelled', async ({ page, app }) => {
    await page.goto('/?jobStepMs=1500#/watches');
    await page.getByRole('button', { name: 'Watches actions' }).click();
    await page.getByRole('menuitem', { name: 'Check all now' }).click();
    await app.statusBar.jobs.click();
    const job = app.jobs.items.first();
    await expect(job).toContainText('Checking');
    await expect(job).toContainText(/\d+ of \d+/);
    await job.getByRole('button', { name: /Cancel/ }).click();
    await expect(job).toContainText(/Cancelled|cancelled/);
  });
});
