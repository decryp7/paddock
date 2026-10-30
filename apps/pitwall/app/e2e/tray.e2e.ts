import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('tray popover', () => {
  test.use({ viewport: { width: 360, height: 520 } });

  test('shows memory, hot models and the key actions, and pauses serving', async ({ page }) => {
    await openApp(page, '/tray');
    await expect(page.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Memory' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Loaded models' })).toContainText('Qwen3');
    await page.getByRole('button', { name: 'Pause serving' }).click();
    await expect(page.getByText(/Paused on/)).toBeVisible();
    await page.getByRole('button', { name: 'Resume' }).click();
    await expect(page.getByText(/Serving on/)).toBeVisible();
    await expect(page.getByRole('button', { name: /Review|pair/ }).first()).toBeVisible();
  });

  // The real popover is 360x520: nothing wraps, nothing is clipped and no loaded model is cut off, with one banner or two.
  for (const query of ['mock-fast&mock-static', 'mock-fast&mock-static&mock-updates']) {
    test(`fits at 360x520 with every action on one line (${query})`, async ({ page }) => {
      await openApp(page, '/tray', query);
      await expect(page.getByRole('region', { name: 'Loaded models' })).toContainText('more loaded');
      const fit = await page.evaluate(() => {
        const box = (e: Element) => e.getBoundingClientRect();
        const tray = document.querySelector('.tray')!;
        const models = document.querySelector('.models')!;
        const buttons = Array.from(document.querySelectorAll('footer button')).map((b) => {
          const label = b.querySelector('.mdc-button__label')!;
          return { text: b.textContent!.trim(), oneLine: box(label).height < 24, inside: box(label).right <= box(b).right && box(label).left >= box(b).left };
        });
        return { tray: [tray.scrollHeight, tray.clientHeight, box(tray).width], models: models.scrollHeight <= models.clientHeight, buttons };
      });
      expect(fit.tray).toEqual([520, 520, 360]);
      expect(fit.models).toBe(true);
      for (const b of fit.buttons) expect(b, b.text).toMatchObject({ oneLine: true, inside: true });
    });
  }
});
