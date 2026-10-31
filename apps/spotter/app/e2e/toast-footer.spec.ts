import { expect, test } from '@decryp7/paddock-e2e';

/** A shell toast never covers the wizard's footer buttons (the toast area keeps the page-footer safe area clear). */
test('the notification toast does not overlap the wizard footer', async ({ page, app }) => {
  // A download that ends while the wizard is open raises a toast over the page.
  await page.goto('/?jobStepMs=50');
  await page.locator('sp-whats-new li.item').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Download' }).click();
  await page.evaluate(() => (location.hash = '#/watches/new'));
  const toast = app.notifications.toastItems.first();
  await expect(toast).toBeVisible({ timeout: 20_000 });
  const footer = page.getByText('Cancel', { exact: true });
  await expect(footer).toBeVisible();
  const [t, b] = await Promise.all([toast.boundingBox(), footer.boundingBox()]);
  const overlap = !!t && !!b && t.x < b.x + b.width && t.x + t.width > b.x && t.y < b.y + b.height && t.y + t.height > b.y;
  expect(overlap, `toast ${JSON.stringify(t)} over Cancel ${JSON.stringify(b)}`).toBe(false);
});
