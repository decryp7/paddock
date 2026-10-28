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

// One toast area: the undo snack bar and the notification toasts stack bottom right and never overlap, at any width (design guide,
// Toasts). The snack bar sits on the host's baseline and the host lifts itself by the snack bar's height (`ToastArea`).
for (const width of [360, 400, 640, 1000, 1280]) {
  test(`the undo toast and the notification toasts do not overlap at ${width} px`, async ({ app, page }) => {
    await page.setViewportSize({ width, height: 800 });
    await app.frame.open();
    await page.getByRole('button', { name: 'Notify three times' }).click();
    await page.getByRole('button', { name: 'Say hello' }).click();
    const snack = page.locator('.mat-mdc-snack-bar-container.pd-undo-snack');
    await expect(snack).toBeVisible();
    await expect(page.getByTestId('notification').first()).toBeVisible();
    // The host's lift follows the snack bar (a resize observer), so wait for the layout to settle before measuring.
    await expect
      .poll(async () => {
        const s = (await snack.boundingBox())!;
        const toasts = page.getByTestId('notification');
        for (let i = 0; i < (await toasts.count()); i++) {
          const t = (await toasts.nth(i).boundingBox())!;
          if (t.x < s.x + s.width && s.x < t.x + t.width && t.y < s.y + s.height && s.y < t.y + t.height) return `toast ${i} ${JSON.stringify(t)} overlaps ${JSON.stringify(s)}`;
        }
        return 'none';
      })
      .toBe('none');
    // Bottom right: the snack bar's right edge is on the notifications' (16 px gutter) and it is at most 360 px wide.
    // (The snack bar scales in for 150 ms, so measure once it has settled.)
    await expect
      .poll(async () => {
        const s = (await snack.locator('.mat-mdc-snackbar-surface').boundingBox())!;
        const t = (await page.getByTestId('notification').first().boundingBox())!;
        return [Math.round(Math.abs(s.x + s.width - (t.x + t.width))), s.width <= 360.5];
      })
      .toEqual([0, true]);
  });
}
