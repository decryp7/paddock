import { axeViolations, expect, installTauriFake, test, THEMES } from '@decryp7/paddock-e2e';

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

    test('a page that appears later keeps its place in the arrow-key order, and labels are 600 selected, 500 otherwise', async ({ app, page }) => {
      await app.frame.open();
      await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
      await expect(app.sidebar.link('Lab')).toHaveCount(0);
      await page.getByRole('button', { name: 'Say hello' }).click();
      await page.getByRole('button', { name: 'Say hello' }).click();
      const names = async () => app.sidebar.links.evaluateAll((els) => els.map((e) => e.getAttribute('data-page-link')));
      await expect.poll(names).toContain('sd.lab');
      const order = await names();
      const keyboard = order.indexOf('sd.keys');
      expect(order[keyboard + 1]).toBe('sd.lab');
      await page.locator('[data-page-link="sd.keys"]').focus();
      await page.keyboard.press('ArrowDown');
      await expect(page.locator('[data-page-link="sd.lab"]')).toBeFocused();
      const weight = (id: string) => page.locator(`[data-page-link="${id}"] .pd-label`).evaluate((el) => getComputedStyle(el).fontWeight);
      expect(await weight('sd.home')).toBe('600');
      expect(await weight('sd.lab')).toBe('500');
    });

    test('a labelled row keeps 8 px or more between icon and name, and its count sits at the row end, clear of the name', async ({ app }) => {
      await app.frame.open();
      await expect(app.sidebar.badges.first()).toBeVisible();
      const row = app.sidebar.link('Runners');
      const box = async (sel: string) => row.locator(sel).first().evaluate((el) => el.getBoundingClientRect().toJSON() as DOMRect);
      const [icon, label, badge, item] = [await box('pd-icon'), await box('.pd-label'), await box('.mat-badge-content'), await row.evaluate((el) => el.getBoundingClientRect().toJSON() as DOMRect)];
      expect(label.left - icon.right).toBeGreaterThanOrEqual(8);
      // The count does not touch the name's text, and ends at the row's right edge (the old pill's place).
      const nameText = await row.locator('.pd-label').evaluate((el) => { const r = document.createRange(); r.selectNodeContents(el); return r.getBoundingClientRect().right; });
      expect(badge.left).toBeGreaterThanOrEqual(nameText);
      expect(item.right - badge.right).toBeLessThanOrEqual(12);
      expect(item.right - badge.right).toBeGreaterThanOrEqual(4);
      // The icon is inset from the row's edge (the accent bar and the focus ring need the room).
      expect(icon.left - item.left).toBeGreaterThanOrEqual(8);
    });

    test('in the rail every icon is centred in its row', async ({ app }) => {
      await app.frame.open();
      await app.frame.toggle.click();
      await expect.poll(() => app.frame.sidebarMode()).toBe('rail');
      const off = await app.sidebar.links.evaluateAll((els) =>
        els.map((a) => {
          const r = a.getBoundingClientRect();
          const i = a.querySelector('pd-icon')!.getBoundingClientRect();
          return Math.abs(i.left + i.width / 2 - (r.left + r.width / 2));
        }),
      );
      expect(Math.max(...off)).toBeLessThanOrEqual(1);
    });

    test('the page lists are lists inside one labelled navigation landmark, and axe (best practice too) finds nothing', async ({ app, page }) => {
      await app.frame.open();
      await expect(page.getByRole('navigation')).toHaveCount(1);
      // The top and the bottom group are each a list (not navigation landmarks of their own) of list items holding the links.
      await expect(page.getByRole('list').filter({ has: page.locator('a[data-page-link]') })).toHaveCount(2);
      await expect(page.getByRole('listitem').filter({ has: page.locator('a[data-page-link]') })).toHaveCount(await app.sidebar.links.count());
      const { default: AxeBuilder } = await import('@axe-core/playwright');
      const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa', 'best-practice']).analyze();
      expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    });

    test('the sidebar and the status bar pass axe, and show no console errors (CSP)', async ({ app, page, pageErrors }) => {
      await app.frame.open();
      await expect(app.sidebar.badges.first()).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  });
}

test.describe('copy menu', () => {
  test('a right click on selected text opens Copy at the pointer, focused; Escape closes it and Copy copies', async ({ app, page, pageErrors }) => {
    await installTauriFake(page, { platform: 'mac', otherwise: 'null' });
    // The page's clipboard is a recorder: no browser permission (WebKit knows no clipboard permissions), nothing on the
    // machine's clipboard (rule 7).
    await page.addInitScript(() => {
      const copied: string[] = [];
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: { writeText: async (t: string) => void copied.push(t), readText: async () => copied.at(-1) ?? '' },
      });
    });
    await app.frame.open();
    const text = page.getByTestId('app-context');
    // Selects the whole line before each right-click: closing the menu may collapse the selection, and a right-click on
    // unselected text then selects the word under the pointer (macOS WebKit), as natively.
    const selectLine = () =>
      text.evaluate((el) => {
        el.classList.add('pd-selectable');
        const r = document.createRange();
        r.selectNodeContents(el);
        getSelection()?.removeAllRanges();
        getSelection()?.addRange(r);
      });
    await selectLine();
    await text.click({ button: 'right', position: { x: 20, y: 8 } });
    const item = page.getByRole('menuitem', { name: 'Copy' });
    await expect(item).toBeVisible();
    await expect(item).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(item).toHaveCount(0);
    await selectLine();
    await text.click({ button: 'right', position: { x: 20, y: 8 } });
    await page.getByRole('menuitem', { name: 'Copy' }).click();
    await expect(page.getByRole('menuitem', { name: 'Copy' })).toHaveCount(0);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('App context');
    // The fake IPC does not answer the shell's version call; that message is the only one expected.
    expect(pageErrors.filter((e) => !/did not say its version/.test(e))).toEqual([]);
  });
});
