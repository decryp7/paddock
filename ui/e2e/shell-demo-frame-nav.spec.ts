import { axeViolations, expect, test, THEMES } from '@decryp7/paddock-e2e';

// Frame navigation on Angular Material and the CDK (Material migration WP-4): the sidebar's arrow keys come from the CDK
// FocusKeyManager, the badges are matBadge, the rail and status bar tooltips are matTooltip, and the copy menu is a CDK
// context menu. Chromium only; console errors are collected so a CSP violation fails the test.
test.use({ consoleErrors: true });

for (const colorScheme of THEMES) {
  test.describe(`frame navigation (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('Up, Down, Home and End move focus through the page links, wrapping, with the focus ring visible', async ({ app, page, pageErrors }) => {
      await app.frame.open();
      const links = app.sidebar.links;
      const count = await links.count();
      await links.first().focus();
      await page.keyboard.press('ArrowDown');
      await expect(links.nth(1)).toBeFocused();
      await page.keyboard.press('End');
      await expect(links.nth(count - 1)).toBeFocused();
      await page.keyboard.press('ArrowDown');
      await expect(links.first()).toBeFocused();
      await page.keyboard.press('ArrowUp');
      await expect(links.nth(count - 1)).toBeFocused();
      await page.keyboard.press('Home');
      await expect(links.first()).toBeFocused();
      // The focus ring is drawn (keyboard focus: :focus-visible).
      expect(await links.first().evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe('none');
      expect(pageErrors).toEqual([]);
    });

    test('the sidebar and the status bar pass axe, and show no console errors (CSP)', async ({ app, page, pageErrors }) => {
      await app.frame.open();
      await expect(app.sidebar.badges.first()).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  });
}
