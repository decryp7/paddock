import { expect, test } from '@playwright/test';

// The Copy menu on selected text (ADR 0008), in Chromium and WebKit. In WebKit (WKWebView on macOS) `contextmenu` comes on the
// button press, so the menu is drawn before the release; the release must not close it, and the selection survives the menu.
test('a right click on selected text opens Copy, focused; the release keeps it open, Escape keeps the selection, Copy copies', async ({ page }) => {
  // A page-local clipboard: no browser permission (WebKit knows only clipboard-read) and never the machine's own clipboard.
  await page.addInitScript(() => {
    let copied = '';
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (t: string) => void (copied = t), readText: async () => copied } });
  });
  await page.goto('/');
  const text = page.locator('h1').first();
  await expect(text).toBeVisible();
  const words = (await text.innerText()).trim();
  await text.evaluate((el) => {
    el.classList.add('pd-selectable'); // selection is opt-in
    const r = document.createRange();
    r.selectNodeContents(el);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(r);
  });
  await text.click({ button: 'right', position: { x: 8, y: 8 } });
  const item = page.getByRole('menuitem', { name: 'Copy' });
  await expect(item).toBeVisible();
  await expect(item).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(item).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => getSelection()?.toString().trim() ?? '')).toBe(words);
  await text.click({ button: 'right', position: { x: 8, y: 8 } });
  await page.getByRole('menuitem', { name: 'Copy' }).click();
  await expect(page.getByRole('menuitem', { name: 'Copy' })).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(words);
});
