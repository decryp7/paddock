import { expect, test, type Page } from '@playwright/test';
import { openApp } from './app';

// The app should feel like a desktop app (ADR 0012): no browser menu on chrome, no reload or page zoom keys, no
// navigating on a dropped file. The guard only prevents the browser's default; Pitwall's own handlers still run.

/** Records, after every other listener, whether the default of the next events of these types was prevented. */
async function watch(page: Page, types: string[]): Promise<void> {
  await page.evaluate((list) => {
    const w = window as unknown as { __feel: Record<string, boolean[]> };
    w.__feel = {};
    for (const type of list) {
      w.__feel[type] = [];
      window.addEventListener(
        type,
        (e) => {
          if (e instanceof KeyboardEvent && ['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
          w.__feel[type]!.push(e.defaultPrevented);
        },
        { passive: true },
      );
    }
  }, types);
}
const prevented = (page: Page, type: string) => page.evaluate((t) => (window as unknown as { __feel: Record<string, boolean[]> }).__feel[t] ?? [], type);

test.describe('native feel', () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test('chrome is not selectable; request text is', async ({ page }) => {
    const selected = () => page.evaluate(() => window.getSelection()?.toString().trim() ?? '');
    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    await page.locator('pw-sidebar nav a').first().dblclick();
    expect(await selected()).toBe('');
    await page.locator('h1').first().dblclick();
    expect(await selected()).toBe('');
    await page.goto('/?mock-fast&mock-static#/requests');
    await page.getByRole('row').nth(1).click();
    const id = page.locator('pw-request-detail pw-fact', { hasText: 'Request id' }).locator('.value');
    await id.dblclick({ position: { x: 8, y: 6 } });
    await expect.poll(selected).not.toBe('');
  });

  test('right-clicking chrome shows no browser menu; text fields keep theirs', async ({ page }) => {
    await watch(page, ['contextmenu']);
    await page.locator('h1').first().click({ button: 'right' });
    await expect.poll(() => prevented(page, 'contextmenu')).toEqual([true]);
    await page.goto('/?mock-fast&mock-static#/models');
    await page.getByRole('tab', { name: /Discover/ }).click();
    await watch(page, ['contextmenu']);
    await page.getByLabel('Search Hugging Face for GGUF models').click({ button: 'right' });
    await expect.poll(async () => (await prevented(page, 'contextmenu')).at(-1)).toBe(false);
  });

  test("right-clicking selected text opens Pitwall's Copy menu; right-clicking elsewhere with a selection does not", async ({ page }) => {
    await page.goto('/?mock-fast&mock-static#/requests');
    await page.getByRole('row').nth(1).click();
    const id = page.locator('pw-request-detail pw-fact', { hasText: 'Request id' }).locator('.value');
    await id.dblclick({ position: { x: 8, y: 6 } });
    await expect.poll(() => page.evaluate(() => window.getSelection()?.toString().trim() ?? '')).not.toBe('');
    await watch(page, ['contextmenu']);
    await page.locator('h1').first().click({ button: 'right' });
    await expect(page.getByRole('menuitem', { name: 'Copy' })).toHaveCount(0);
    await id.click({ button: 'right', position: { x: 6, y: 6 } });
    await expect(page.getByRole('menuitem', { name: 'Copy', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menuitem', { name: 'Copy' })).toHaveCount(0);
  });

  test('reload, print and zoom keys are blocked and the window stays', async ({ page }) => {
    await page.evaluate(() => ((window as unknown as { __marker: number }).__marker = 42));
    await watch(page, ['keydown']);
    await page.locator('h1').first().click();
    await page.keyboard.press('F5');
    // The platform modifier follows the user agent: Cmd on macOS, Ctrl elsewhere.
    const mod = (await page.evaluate(() => /Mac OS X|Macintosh/.test(navigator.userAgent))) ? 'Meta' : 'Control';
    await page.keyboard.press(`${mod}+r`);
    await page.keyboard.press(`${mod}+p`);
    await page.keyboard.press(`${mod}+Equal`);
    await expect.poll(async () => {
      const seen = await prevented(page, 'keydown');
      return seen.length >= 3 && seen.every(Boolean);
    }).toBe(true);
    await page.waitForTimeout(400);
    expect(await page.evaluate(() => (window as unknown as { __marker?: number }).__marker)).toBe(42);
  });

  test('a dropped file never navigates, and links cannot be dragged out', async ({ page }) => {
    const dropped = await page.evaluate(() => {
      const data = new DataTransfer();
      data.items.add(new File(['x'], 'a.log', { type: 'text/plain' }));
      const target = document.querySelector('h1')!;
      const over = new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: data });
      target.dispatchEvent(over);
      const drop = new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: data });
      target.dispatchEvent(drop);
      const a = document.createElement('a');
      a.href = '#x';
      document.body.append(a);
      const drag = new DragEvent('dragstart', { bubbles: true, cancelable: true });
      a.dispatchEvent(drag);
      a.remove();
      return [over.defaultPrevented, drop.defaultPrevented, drag.defaultPrevented];
    });
    expect(dropped).toEqual([true, true, true]);
  });

  test('uses the OS UI font and shows focus rings for the keyboard only', async ({ page }) => {
    const font = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
    expect(font).toContain('system-ui');
    expect(font).not.toContain('Inter');
    await page.getByRole('link', { name: /Models/ }).first().click();
    const mouseOutline = await page.getByRole('tab', { name: 'Library' }).evaluate((el) => getComputedStyle(el).outlineStyle);
    expect(mouseOutline).toBe('none');
    await page.keyboard.press('Tab');
    const focused = await page.evaluate(() => getComputedStyle(document.activeElement!).outlineStyle);
    expect(focused).toBe('solid');
  });
});
