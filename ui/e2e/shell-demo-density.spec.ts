import { expect, test } from '@decryp7/paddock-e2e';

// ER-21: the shared Material surfaces keep the family's control height and shape even when the host app's own Material theme sets
// other values on `html` (Pitcrew's `mat.theme` at density 0 made fields 56px tall with large type and buttons pills). The
// hostile theme below is what such an app declares; the shared surfaces must not follow it.

const HOSTILE = `:root:root { --mat-form-field-container-height: 56px; --mat-form-field-container-vertical-padding: 16px;
  --mat-form-field-container-text-size: 20px; --mat-select-trigger-text-size: 20px;
  --mat-button-outlined-container-height: 48px; --mat-button-outlined-container-shape: 24px; --mat-button-filled-container-height: 48px;
  --mat-button-filled-container-shape: 24px; --mat-button-text-container-height: 48px; --mat-button-toggle-height: 56px; --mat-button-toggle-shape: 28px; }`;

async function controlHeight(page: import('@playwright/test').Page): Promise<number> {
  return page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--pd-control-height')) || 0);
}

test.describe('shared settings and panels keep the family density (ER-21)', () => {
  test('Appearance: selects and Reset section stay at the family height and shape under a host theme', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.appearance' });
    await page.addStyleTag({ content: HOSTILE });
    const limit = await controlHeight(page);
    expect(limit).toBeGreaterThan(0);
    for (const key of ['theme', 'contrast', 'motion', 'density']) {
      const box = await app.settings.field(key).locator('mat-form-field').boundingBox();
      expect(box!.height, key).toBeLessThanOrEqual(limit + 1);
    }
    const reset = page.getByRole('button', { name: 'Reset section' });
    const radius = await reset.evaluate((el) => parseFloat(getComputedStyle(el).borderTopLeftRadius));
    expect((await reset.boundingBox())!.height).toBeLessThanOrEqual(36);
    expect(radius).toBeLessThan(12);
  });

  test('Notification channels: fields and buttons stay at the family height too', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.channels' }, '?channels=empty');
    await page.addStyleTag({ content: HOSTILE });
    await page.getByTestId('channel-add-empty').click();
    const limit = await controlHeight(page);
    expect((await page.locator('#pd-ch-type').boundingBox())!.height).toBeLessThanOrEqual(limit + 1);
    expect((await page.getByTestId('channel-save').boundingBox())!.height).toBeLessThanOrEqual(36);
  });

  test('the notification centre’s All / Unread control stays at the family height', async ({ app, page }) => {
    await page.goto('/');
    await app.statusBar.bell.click();
    await page.addStyleTag({ content: HOSTILE });
    const group = page.getByRole('radiogroup').or(page.locator('mat-button-toggle-group')).first();
    await expect(group).toBeVisible();
    expect((await group.boundingBox())!.height).toBeLessThanOrEqual(40);
  });
});
