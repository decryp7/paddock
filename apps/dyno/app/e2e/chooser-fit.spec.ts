import { expect, test } from '@decryp7/paddock-e2e';

/**
 * The "Add a widget" list shows every row whole: no row is cut through its description and the list does not scroll inside the page
 * (a nested scroll with a half row at its foot was easy to miss and read as a clipped list). Waits are on conditions, never sleeps.
 */
test('the dashboard chooser shows every row of its add list whole', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/?latency=0#/');
  await page.getByRole('button', { name: 'Customize dashboard' }).click();
  const list = page.locator('ul.pd-wc-addlist');
  await expect(list).toBeVisible();
  await expect(list.locator('li')).not.toHaveCount(0);
  const clipped = await list.evaluate((ul) => {
    const box = ul.getBoundingClientRect();
    const rows = [...ul.querySelectorAll('li')].map((li) => li.getBoundingClientRect());
    return {
      scrolls: ul.scrollHeight - ul.clientHeight,
      rowsOutside: rows.filter((r) => r.bottom > box.bottom + 1 || r.top < box.top - 1).length,
      rows: rows.length,
    };
  });
  expect(clipped.rows).toBeGreaterThan(8);
  expect(clipped.scrolls, 'the list scrolls inside the page').toBeLessThanOrEqual(1);
  expect(clipped.rowsOutside, 'a row is cut by the list box').toBe(0);
});
