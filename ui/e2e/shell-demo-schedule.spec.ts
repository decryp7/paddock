import { axeViolations, expect, expectNoHorizontalScroll, LAYOUT_WIDTHS, test, THEMES } from '@decryp7/paddock-e2e';

// The shared schedule editors (plan X33) in the shell demo: modes, presets, times, cron problems, the preview from the
// demo's stand-in engine (a fixed demo clock, so nothing here depends on the real time), pause windows, axe in both themes
// and no horizontal scroll at any width. Only the test kit (@decryp7/paddock-e2e).


test.describe('schedule editors (plan X33)', () => {
  test('the page lists a schedule and pause windows and the editor switches between the three modes', async ({ app, page, pageErrors }) => {
    await app.frame.open('#/schedule');
    await expect(app.frame.heading).toHaveText('Schedules');
    await expect(page.getByTestId('schedule-now')).toHaveText('The demo clock stands at 2026-10-22T08:00:00.000Z.');
    const modes = page.getByRole('radiogroup', { name: 'Schedule type' }).getByRole('radio');
    await expect(modes).toHaveText(['Every…', 'At times', 'Cron']);
    await expect(page.getByRole('radio', { name: 'Every…' })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByTestId('schedule-preview').locator('.pd-runs li')).toHaveCount(5);

    await page.getByRole('radio', { name: 'At times' }).click();
    const days = page.getByTestId('schedule-times');
    await expect(days.getByRole('button', { name: 'Mon', exact: true })).toHaveAttribute('aria-pressed', 'true');
    await expect(days.getByRole('button', { name: 'Sat', exact: true })).toHaveAttribute('aria-pressed', 'false');
    await expect(page.getByTestId('schedule-preview')).toContainText('5 days at 09:00');
    await page.getByRole('radio', { name: 'Cron' }).click();
    await expect(page.getByTestId('schedule-cron').getByRole('textbox').first()).toHaveValue('0 9 * * 1-5');
    expect(pageErrors).toEqual([]);
  });

  test('presets and the amount change the interval; a cron with the wrong number of fields shows the engine’s problem', async ({ app, page }) => {
    await app.frame.open('#/schedule');
    await page.getByRole('radio', { name: '15 min' }).click();
    await expect(page.getByRole('radio', { name: '15 min' })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByTestId('schedule-preview')).toContainText('Every 15 minutes');
    const amount = page.getByTestId('schedule-every').locator('input[type=number]');
    await expect(amount).toHaveValue('15');
    await amount.fill('45');
    await amount.press('Tab');
    await expect(page.getByTestId('schedule-preview')).toContainText('Every 45 minutes');
    await expect(page.getByRole('radio', { name: '15 min' })).toHaveAttribute('aria-checked', 'false');

    await page.getByRole('radio', { name: 'Cron' }).click();
    const expr = page.getByTestId('schedule-cron').getByRole('textbox').first();
    await expr.fill('* * *');
    await expr.press('Tab');
    await expect(page.getByTestId('schedule-preview').getByRole('alert')).toContainText('five fields');
    await expect(page.getByTestId('schedule-preview').locator('.pd-runs')).toHaveCount(0);
    await expr.fill('*/5 * * * *');
    await expr.press('Tab');
    await expect(page.getByTestId('schedule-preview').getByRole('alert')).toHaveCount(0);
  });

  test('times of day are added and removed, and the last one cannot be removed', async ({ app, page }) => {
    await app.frame.open('#/schedule');
    await page.getByRole('radio', { name: 'At times' }).click();
    const remove = page.getByRole('button', { name: /^Remove time of day/ });
    await expect(remove).toBeDisabled();
    await page.getByRole('button', { name: 'Add a time' }).click();
    await expect(page.getByRole('combobox', { name: 'Time of day 2' })).toHaveValue('10:00 AM');
    await expect(remove.first()).toBeEnabled();
    await remove.first().click();
    await expect(page.getByRole('combobox', { name: /^Time of day/ })).toHaveCount(1);
    // With no day picked the engine says what to do.
    for (const day of ['Mon', 'Tue', 'Wed', 'Thu', 'Fri']) await page.getByTestId('schedule-times').getByRole('button', { name: day, exact: true }).click();
    await expect(page.getByTestId('schedule-preview').getByRole('alert')).toHaveText('Pick at least one day.');
  });

  test('pause windows: add, edit, overnight note, remove, and the engine’s problem for an empty window', async ({ app, page }) => {
    await app.frame.open('#/schedule');
    const first = page.getByTestId('pause-window-0');
    await expect(first).toContainText('Ends the next day.');
    await page.getByRole('button', { name: 'Add a window' }).click();
    await expect(page.getByTestId('pause-window-1')).toBeVisible();
    // The timepicker takes the time in the locale's words and commits on leaving the field.
    const times = page.getByTestId('pause-window-1').getByRole('combobox');
    await times.nth(0).fill('9:00 AM');
    await times.nth(0).press('Tab');
    await times.nth(1).fill('9:00 AM');
    await times.nth(1).press('Tab');
    await expect(page.getByRole('alert')).toContainText('differs from its end');
    await times.nth(1).fill('10:00 AM');
    await times.nth(1).press('Tab');
    await expect(page.getByRole('alert')).toHaveCount(0);
    await page.getByRole('button', { name: 'Remove window 2' }).click();
    await expect(page.getByTestId('pause-window-1')).toHaveCount(0);
    await page.getByRole('button', { name: 'Remove window 1' }).click();
    await expect(page.getByText('No windows: nothing is paused.')).toBeVisible();
  });

  test('every icon button is at least 14 px and 24 px to hit', async ({ app, page }) => {
    await app.frame.open('#/schedule');
    await page.getByRole('radio', { name: 'At times' }).click();
    await page.getByRole('button', { name: 'Add a time' }).click();
    for (const button of await page.getByRole('button', { name: /^Remove / }).all()) {
      const box = (await button.boundingBox())!;
      expect(box.width).toBeGreaterThanOrEqual(24);
      expect(box.height).toBeGreaterThanOrEqual(24);
      const icon = (await button.locator('svg').boundingBox())!;
      expect(icon.width).toBeGreaterThanOrEqual(14);
    }
  });

  for (const width of LAYOUT_WIDTHS) {
    test(`no horizontal scroll at ${width} px in any mode`, async ({ app, page }) => {
      await page.setViewportSize({ width, height: 700 });
      await app.frame.open('#/schedule');
      for (const mode of ['Every…', 'At times', 'Cron']) {
        await page.getByRole('radio', { name: mode }).click();
        await expectNoHorizontalScroll(page, `${mode} at ${width}`);
      }
    });
  }

  for (const colorScheme of THEMES) {
    test(`axe finds nothing in any mode (${colorScheme})`, async ({ app, page }) => {
      await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
      await app.frame.open('#/schedule');
      for (const mode of ['Every…', 'At times', 'Cron']) {
        await page.getByRole('radio', { name: mode }).click();
        expect(await axeViolations(page), mode).toEqual([]);
      }
    });
  }
});
