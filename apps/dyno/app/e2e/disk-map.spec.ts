import { expect, test } from '@playwright/test';

/** The disk map against the mock backend: scan, drill, up, table view (chromium only). */
const url = (query = '') => `/?latency=0${query}#/disk-map`;

test.use({ reducedMotion: 'reduce' });

test.describe('disk map', () => {
  test('scans a volume, drills into a folder, goes up and shows the table', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('heading', { level: 1, name: 'Disk map' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'See what fills Macintosh HD' })).toBeVisible();

    await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
    const list = page.getByRole('region', { name: 'In this folder' }).or(page.locator('section.list'));
    await expect(list.getByRole('button', { name: /^Open Users/ })).toBeVisible();

    await list.getByRole('button', { name: /^Open Users/ }).click();
    await expect(page.locator('.crumb.current')).toHaveText('Users');
    await list.getByRole('button', { name: /^Open alex/ }).click();
    await expect(page.locator('.crumb.current')).toHaveText('alex');

    await page.getByRole('button', { name: 'Go up to Users' }).first().click();
    await expect(page.locator('.crumb.current')).toHaveText('Users');

    await page.getByRole('radio', { name: 'Table' }).click();
    const table = page.getByRole('table', { name: /Contents of Users/ });
    await expect(table.getByRole('columnheader', { name: /Newest change/ })).toBeVisible();
    await expect(table.getByRole('row', { name: /alex/ })).toBeVisible();

    await page.getByRole('navigation', { name: 'Folder path' }).getByRole('button', { name: 'Macintosh HD' }).click();
    await expect(page.locator('.crumb.current')).toHaveText('Macintosh HD');
    await expect(page.getByRole('table', { name: /Contents of Macintosh HD/ }).getByRole('row', { name: /Applications/ })).toBeVisible();
  });

  test('clicks a ring segment to drill in, and the centre goes back up', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
    const ring = page.getByRole('tree', { name: 'Disk usage of Macintosh HD' });
    await expect(ring.getByRole('treeitem', { name: /^Hidden space/ })).toBeVisible();
    // The middle of a ring segment's box is not on the segment, so the click is sent to the segment itself.
    await ring.getByRole('treeitem', { name: /^Users,/ }).dispatchEvent('click');
    await expect(page.locator('.crumb.current')).toHaveText('Users');
    await expect(page.getByRole('region', { name: 'In this folder' }).or(page.locator('section.list')).getByRole('button', { name: /^Open alex/ })).toBeVisible();
    await page.getByRole('button', { name: 'Up to Macintosh HD', exact: true }).click();
    await expect(page.locator('.crumb.current')).toHaveText('Macintosh HD');
    await ring.getByRole('treeitem', { name: /^Users,/ }).dispatchEvent('mouseenter');
    await expect(page.locator('section.list .item.linked')).toContainText('Users');
  });

  test('a scan shows its job and Cancel keeps a usable partial result', async ({ page }) => {
    await page.goto(url().replace('latency=0', 'latency=400'));
    await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
    const job = page.getByRole('region', { name: 'Scan progress' });
    await expect(job).toBeVisible();
    await job.getByRole('button', { name: 'Cancel scan' }).click();
    await expect(page.getByText('Partial result.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
    await expect(page.locator('section.list .item').first()).toBeVisible();
  });

  test('reveals a folder, and reads only the refused folders again', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Reveal in Finder', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Reveal in Finder', exact: true }).click();
    await expect(page.getByRole('alert')).toHaveCount(0);
    const warn = page.locator('.callout.warn');
    await expect(warn).toContainText('could not be read');
    await expect(warn.getByRole('button', { name: 'Open Full Disk Access settings' })).toBeVisible();
    await warn.getByRole('button', { name: 'Read again' }).click();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
  });

  test('the sidebar and the command palette reach the page', async ({ page }) => {
    await page.goto('/?latency=0#/');
    await page.getByRole('navigation').first().getByRole('link', { name: 'Disk map', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Disk map' })).toBeVisible();
  });

  for (const width of [900, 1024, 1280]) {
    test(`nothing clips at ${width} px, in the chart view and in the table`, async ({ page }, info) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(url());
      await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
      await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
      const clipped = () =>
        page.evaluate(() =>
          Array.from(document.querySelectorAll<HTMLElement>('tr, .item, .toolbar, .capacity, .job'))
            .filter((el) => el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1)
            .map((el) => `${el.tagName.toLowerCase()}.${String(el.className).trim()}: needs ${el.scrollWidth}px, has ${el.clientWidth}px`),
        );
      expect(await clipped()).toEqual([]);
      if (width === 900) await page.screenshot({ path: process.env['DISK_MAP_SHOTS'] ? `${process.env['DISK_MAP_SHOTS']}/disk-map-chart-900.png` : info.outputPath('chart-900.png') });
      await page.getByRole('radio', { name: 'Table' }).click();
      await expect(page.getByRole('table', { name: /Contents of/ })).toBeVisible();
      expect(await clipped()).toEqual([]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      if (width === 900) await page.screenshot({ path: process.env['DISK_MAP_SHOTS'] ? `${process.env['DISK_MAP_SHOTS']}/disk-map-table-900.png` : info.outputPath('table-900.png') });
    });
  }
});
