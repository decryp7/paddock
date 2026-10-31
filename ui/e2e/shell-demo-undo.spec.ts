import { axeViolations, expect, expectNoHorizontalScroll, LAYOUT_WIDTHS, pressMod, test, THEMES } from '@decryp7/paddock-e2e';

// Undo and the activity log (plan X44) in the shell demo: a greeting is an undoable edit (Edit > Undo and Redo keys, the
// "Done, Undo" toast) and an entry of the Activity page, which can take it back. The Lab page exists while there is at least
// one greeting, which makes the state visible. Nothing here waits for a timer: the toast is dismissed by its own button.

test.describe('undo and activity (plan X44)', () => {
  test('a greeting announces a toast whose Undo takes it back, and Mod+Z and Mod+Shift+Z undo and redo', async ({ app, page, pageErrors }) => {
    await app.frame.open();
    await expect(app.sidebar.link('Lab')).toHaveCount(0);
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(app.sidebar.link('Lab')).toBeVisible();
    const toast = page.getByTestId('undo-toast');
    await expect(toast).toContainText('Done: greeting');
    await toast.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(app.sidebar.link('Lab')).toHaveCount(0);
    await expect(toast).toHaveCount(0);

    await pressMod(page, 'Mod+Shift+z');
    await expect(app.sidebar.link('Lab')).toBeVisible();
    await pressMod(page, 'Mod+z');
    await expect(app.sidebar.link('Lab')).toHaveCount(0);

    // The close button hides the toast and keeps the edit.
    await page.getByRole('button', { name: 'Say hello' }).click();
    await page.getByTestId('undo-toast').getByRole('button', { name: 'Dismiss' }).click();
    await expect(page.getByTestId('undo-toast')).toHaveCount(0);
    await expect(app.sidebar.link('Lab')).toBeVisible();
    expect(pageErrors).toEqual([]);
  });

  test('the toast is a Material snack bar: announced politely, it takes no focus and keeps the page-footer safe area', async ({ app, page }) => {
    await app.frame.open();
    await page.getByRole('button', { name: 'Say hello' }).click();
    const toast = page.getByTestId('undo-toast');
    await expect(toast).toBeVisible();
    // The snack bar moves the content into its polite live region a moment after opening.
    await expect(page.locator('.mat-mdc-snack-bar-container [aria-live="polite"]').getByTestId('undo-toast')).toBeVisible();
    expect(await toast.evaluate((e) => e.contains(document.activeElement))).toBe(false);
    // Clear of the status bar and the page footer (guide, Toasts): its bottom edge is above the safe area.
    const safe = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      return parseFloat(root.getPropertyValue('--pd-statusbar-height')) + parseFloat(root.getPropertyValue('--pd-page-footer-safe'));
    });
    const box = (await page.locator('.mat-mdc-snack-bar-container.pd-undo-snack').boundingBox())!;
    expect(box.y + box.height).toBeLessThanOrEqual(page.viewportSize()!.height - safe);
  });

  test('with reduced motion the toast does not animate', async ({ app, page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await app.frame.open();
    await page.getByRole('button', { name: 'Say hello' }).click();
    const container = page.locator('.mat-mdc-snack-bar-container.pd-undo-snack');
    await expect(container).toBeVisible();
    const style = await container.evaluate((e) => {
      const cs = getComputedStyle(e);
      return { opacity: cs.opacity, name: cs.animationName, seconds: parseFloat(cs.animationDuration) };
    });
    expect(style.opacity).toBe('1');
    expect(style.name === 'none' || style.seconds <= 0.001).toBe(true);
  });

  test('the Activity page lists the greeting and Undo there takes it back for good', async ({ app, page }) => {
    await app.frame.open();
    await app.sidebar.link('Activity').click();
    await expect(page.getByText('Nothing yet')).toBeVisible();
    await app.sidebar.link('Overview').click();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(app.sidebar.link('Lab')).toBeVisible();
    await app.sidebar.link('Activity').click();
    const row = page.getByTestId('activity-table').getByRole('row').nth(1);
    await expect(row).toContainText('Said hello (1)');
    await expect(row).toContainText('From the Say hello command');
    await row.getByRole('button', { name: 'Undo: Said hello (1)' }).click();
    await expect(row).toContainText('Undone');
    await expect(row.getByRole('button')).toHaveCount(0);
    await expect(app.sidebar.link('Lab')).toHaveCount(0);
  });

  test('every icon button is at least 14 px', async ({ app, page }) => {
    await app.frame.open();
    await page.getByRole('button', { name: 'Say hello' }).click();
    // The snack bar scales in for 150 ms, which changes a bounding box: wait for the live region, and read the layout width
    // (transforms do not affect it).
    await expect(page.locator('[aria-live="polite"]').getByTestId('undo-toast')).toBeVisible();
    const width = await page.getByTestId('undo-toast').locator('svg').evaluate((e) => parseFloat(getComputedStyle(e).width));
    expect(width).toBeGreaterThanOrEqual(14);
  });

  for (const width of LAYOUT_WIDTHS) {
    test(`no horizontal scroll at ${width} px on the Activity page`, async ({ app, page }) => {
      await page.setViewportSize({ width, height: 700 });
      await app.frame.open();
      await page.getByRole('button', { name: 'Say hello' }).click();
      await app.frame.open('#/activity');
      await expectNoHorizontalScroll(page, `activity at ${width}`);
    });
  }

  for (const colorScheme of THEMES) {
    test(`axe finds nothing on the Activity page and with the toast (${colorScheme})`, async ({ app, page }) => {
      await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
      await app.frame.open();
      await page.getByRole('button', { name: 'Say hello' }).click();
      await expect(page.getByTestId('undo-toast')).toBeVisible();
      // The snack bar holds its content aria-hidden for a moment, then moves it into its live region (that is the announcement).
      await expect(page.locator('[aria-live="polite"]').getByTestId('undo-toast')).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      await app.sidebar.link('Activity').click();
      await expect(page.getByTestId('activity-table')).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
    });
  }
});
