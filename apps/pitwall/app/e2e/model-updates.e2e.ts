import { expect, test } from '@playwright/test';
import { openSettings, settingsDialog } from './app';

test.describe('model updates', () => {
  test('is off until you turn it on; Check now finds updates, which only ever download after you confirm', async ({ page }) => {
    await openSettings(page);
    const panel = settingsDialog(page).locator('#set-model-updates');
    await expect(panel).toBeVisible();
    await expect(panel.getByRole('switch', { name: 'Check for model updates' })).not.toBeChecked();
    await expect(panel).toContainText('only the repository ids');
    await expect(panel).toContainText('Never checked.');

    await panel.getByRole('button', { name: 'Check now' }).click();
    await expect(page.getByText('Checked: 2 updates available.')).toBeVisible();
    await expect(panel).toContainText('Last checked');

    // The dashboard points at them; Models lists them and marks the rows.
    await settingsDialog(page).getByRole('button', { name: 'Close' }).first().click();
    await expect(settingsDialog(page)).toHaveCount(0);
    const summary = page.locator('pw-update-banner .summary');
    await expect(summary).toContainText('2 model updates are available');
    await summary.getByRole('link', { name: 'Review' }).click();
    const banner = page.locator('pw-update-banner .banner');
    await expect(banner).toContainText('2 updates available for models you downloaded');
    await expect(page.locator('pw-library-tab tbody mat-icon.upd')).toHaveCount(2);
    await expect(page.locator('pw-library-tab tbody mat-icon.upd').first()).toHaveAttribute('aria-label', 'Update available for Qwen3.5 9B');

    // Get it names the host and the size; cancelling downloads nothing.
    await banner.getByRole('button', { name: 'Get Qwen3.5-9B-Q4_K_M.gguf' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText('from huggingface.co');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(banner).toContainText('2 updates available');
    await page.getByRole('tab', { name: /Downloads/ }).click();
    await expect(page.locator('pw-downloads-list')).not.toContainText('Qwen3.5-9B-Q4_K_M.gguf');
    await page.getByRole('tab', { name: /Library/ }).click();

    // Confirming starts the normal download and clears that notice.
    await banner.getByRole('button', { name: 'Get Qwen3.5-9B-Q4_K_M.gguf' }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Download update' }).click();
    await expect(page.getByText('Downloading Qwen3.5-9B-Q4_K_M.gguf.')).toBeVisible();
    await expect(banner).toContainText('1 update available');
    await page.getByRole('tab', { name: /Downloads/ }).click();
    await expect(page.locator('pw-downloads-list')).toContainText('Qwen3.5-9B-Q4_K_M.gguf');
    await page.getByRole('tab', { name: /Library/ }).click();

    // Dismissing the last one removes the banner and the row note.
    await banner.getByRole('button', { name: /^Dismiss the update for qwen3.5-4b/ }).click();
    await expect(page.locator('pw-update-banner .banner')).toHaveCount(0);
    await expect(page.locator('pw-library-tab tbody mat-icon.upd')).toHaveCount(0);
  });

  test('turns the schedule on and off', async ({ page }) => {
    await openSettings(page);
    const panel = settingsDialog(page).locator('#set-model-updates');
    const toggle = panel.getByRole('switch', { name: 'Check for model updates' });
    await toggle.click();
    await expect(toggle).toBeChecked();
    await expect(panel).toContainText('The next check runs');
    await panel.getByRole('combobox', { name: /^How often/ }).click();
    await page.getByRole('option', { name: 'Every day' }).click();
    await expect(panel.getByRole('combobox', { name: /^How often/ })).toContainText('Every day');
    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await expect(panel.getByRole('combobox', { name: /^How often/ })).toBeDisabled();
  });
});
