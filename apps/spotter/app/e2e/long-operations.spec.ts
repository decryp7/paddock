import { expect, test } from '@playwright/test';

/**
 * Long operations against the mock backend (which paces its steps like the shell would report them): the panel shows
 * the phase in words with its own numbers from the start, Cancel works, and the UI never gives up on its own.
 */
test.describe('long operations show what they are doing and can be cancelled', () => {
  test('a download shows its phase and bytes, and Cancel stops it', async ({ page }) => {
    await page.goto('/');
    await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Download' }).click();
    const panel = page.getByRole('region', { name: 'Running operations' });
    await expect(panel).toBeVisible();
    await expect(panel.locator('.phase')).toHaveText('Downloading');
    await expect(panel.locator('.numbers')).toContainText(/MiB of 176 MiB/);
    await expect(panel.locator('progress')).toHaveAttribute('max', '1');
    await panel.getByRole('button', { name: /^Cancel: / }).click();
    await expect(page.getByText('Download cancelled')).toBeVisible();
    await expect(panel).toHaveCount(0);
    await expect(page.getByText('Download failed')).toHaveCount(0);
  });

  test('a download that is left alone finishes and says where the file is', async ({ page }) => {
    await page.goto('/');
    await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Download' }).click();
    await expect(page.getByText('Download finished')).toBeVisible({ timeout: 20_000 });
    await expect(page.getByRole('region', { name: 'Running operations' })).toHaveCount(0);
  });

  test('installing the browser goes through Downloading, Unpacking and Testing', async ({ page }) => {
    await page.goto('/#/settings');
    await page.getByRole('button', { name: 'Download Chromium' }).click();
    const panel = page.getByRole('region', { name: 'Running operations' });
    const phase = panel.locator('.phase');
    await expect(phase).toHaveText('Downloading');
    await expect(phase).toHaveText('Unpacking', { timeout: 10_000 });
    await expect(phase).toHaveText('Testing', { timeout: 10_000 });
    await expect(page.getByText('Browser installed')).toBeVisible({ timeout: 10_000 });
  });

  test('checking every watch shows n of m and can be cancelled', async ({ page }) => {
    await page.goto('/#/watches');
    await page.getByRole('button', { name: 'Watches actions' }).click();
    await page.getByRole('menuitem', { name: 'Check all now' }).click();
    const panel = page.getByRole('region', { name: 'Running operations' });
    await expect(panel).toBeVisible();
    await expect(panel.locator('.phase')).toHaveText('Checking');
    await expect(panel.locator('.numbers')).toHaveText(/^\d+ of \d+$/);
    await panel.getByRole('button', { name: /^Cancel: / }).click();
    await expect(page.getByText('Check cancelled')).toBeVisible();
    await expect(panel).toHaveCount(0);
  });
});
