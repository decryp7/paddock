import { expect, test } from '@decryp7/paddock-e2e';

// A toast never covers a page's footer action (a wizard's Next): the host keeps the page-footer safe area above the
// status bar (design guide, Toasts). The footer is a stand-in a page would render at the bottom of its area. Chromium only
// in branches (rule 3).

test('a toast does not overlap a primary button in the page footer', async ({ app, page }) => {
  await app.frame.open();
  await page.getByRole('region', { name: 'Notifications' }).waitFor({ state: 'attached' });
  await page.evaluate(() => {
    const main = document.querySelector('[data-pd-region="main"]') as HTMLElement;
    const footer = document.createElement('footer');
    footer.setAttribute('data-testid', 'fake-footer');
    footer.style.cssText = 'position:sticky;bottom:0;display:flex;justify-content:flex-end;padding:8px 16px;';
    const next = document.createElement('button');
    next.textContent = 'Next';
    next.setAttribute('data-testid', 'fake-next');
    footer.append(next);
    main.append(footer);
  });
  await page.getByRole('button', { name: 'Notify three times' }).click();
  const toast = page.getByTestId('notification').last();
  await expect(toast).toBeVisible();
  const [t, b] = [await toast.boundingBox(), await page.getByTestId('fake-next').boundingBox()];
  expect(t).not.toBeNull();
  expect(b).not.toBeNull();
  const overlap = t!.x < b!.x + b!.width && b!.x < t!.x + t!.width && t!.y < b!.y + b!.height && b!.y < t!.y + t!.height;
  expect(overlap, `toast ${JSON.stringify(t)} covers Next ${JSON.stringify(b)}`).toBe(false);
});
