import { clickShape, expect, hoverShape, test } from '@decryp7/paddock-e2e';

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
    await clickShape(page, ring.getByRole('treeitem', { name: /^Users,/ }));
    await expect(page.locator('.crumb.current')).toHaveText('Users');
    await expect(page.getByRole('region', { name: 'In this folder' }).or(page.locator('section.list')).getByRole('button', { name: /^Open alex/ })).toBeVisible();
    await page.getByRole('button', { name: 'Up to Macintosh HD', exact: true }).click();
    await expect(page.locator('.crumb.current')).toHaveText('Macintosh HD');
    await hoverShape(page, ring.getByRole('treeitem', { name: /^Users,/ }));
    await expect(ring.getByRole('treeitem', { name: /^Users,/ })).toHaveClass(/pd-arc--active/);
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

  test('a saved map opens at once, stays during a Rescan and is swapped when the new pass is done', async ({ page }) => {
    // The rescan's pass is held where it starts until the test lets it go: no timing to guess.
    await page.goto(url('&saved=3&gate=scans'));
    await expect(page.getByText('scanned 3 days ago (saved)')).toBeVisible();
    const note = page.getByTestId('saved-map');
    await expect(note).toContainText('Net change in used space since: +12.4 GB');
    await expect(note).toContainText('a net figure');
    const list = page.getByRole('region', { name: 'In this folder' }).or(page.locator('section.list'));
    await expect(list.getByRole('button', { name: /^Open Users/ })).toBeVisible();

    await page.getByRole('button', { name: 'Rescan Macintosh HD' }).click();
    // The old map stays on screen with the rescan's progress above it.
    await expect(page.getByRole('region', { name: 'Rescan progress' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Rescan progress' })).toContainText('the map below stays until this is done');
    await expect(list.getByRole('button', { name: /^Open Users/ })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeDisabled();

    // Done: the page swaps to the new map and the saved note goes.
    await page.evaluate(() => (window as unknown as { __dynoReleaseScans: () => void }).__dynoReleaseScans());
    await expect(page.getByRole('region', { name: 'Rescan progress' })).toBeHidden();
    await expect(page.getByTestId('saved-map')).toBeHidden();
    await expect(page.getByText('(saved)')).toBeHidden();
    await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeEnabled();
  });

  test('a cancelled Rescan keeps the saved map, and Forget this map clears the page', async ({ page }) => {
    await page.goto(url('&saved=2&gate=scans'));
    await expect(page.getByTestId('saved-map')).toBeVisible();
    await page.getByRole('button', { name: 'Rescan Macintosh HD' }).click();
    await page.getByRole('region', { name: 'Rescan progress' }).getByRole('button', { name: 'Cancel rescan' }).click();
    await expect(page.getByRole('region', { name: 'Rescan progress' })).toBeHidden();
    await expect(page.getByTestId('saved-map')).toBeVisible();
    await page.getByRole('button', { name: 'Forget this map' }).click();
    await expect(page.getByRole('heading', { name: 'See what fills Macintosh HD' })).toBeVisible();
  });

  test('the sidebar and the command palette reach the page', async ({ page }) => {
    await page.goto('/?latency=0#/');
    await page.getByRole('navigation').first().getByRole('link', { name: 'Disk map', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Disk map' })).toBeVisible();
  });

  test('the volume picker is a Material menu with a surface behind it', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('button', { name: 'Choose a volume' }).click();
    const panel = page.locator('.mat-mdc-menu-panel');
    await expect(panel).toBeVisible();
    await expect(panel.getByRole('menuitem', { name: /Choose a folder/ })).toBeVisible();
    const alpha = await panel.evaluate((el) => {
      const c = getComputedStyle(el).backgroundColor;
      const m = /rgba?\(([^)]+)\)/.exec(c);
      const parts = m?.[1]?.split(/[ ,/]+/).filter(Boolean) ?? [];
      return parts.length >= 4 ? Number(parts[3]) : 1;
    });
    expect(alpha).toBeGreaterThan(0.9);
    await page.keyboard.press('Escape');
    await expect(panel).toBeHidden();
  });

  for (const width of [900, 1024, 1280]) {
    test(`nothing clips at ${width} px, in the chart view and in the table`, async ({ page }, info) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(url());
      await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
      await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
      // Every row (list rows, table rows), the toolbar, the capacity strip and the job clip nothing, and the row actions sit inside their row.
      const clipped = () =>
        page.evaluate(() => {
          const out: string[] = [];
          for (const el of Array.from(document.querySelectorAll<HTMLElement>('tr, .item, .toolbar, .capacity, .job'))) {
            if (el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1) out.push(`${el.tagName.toLowerCase()}.${String(el.className).trim()}: needs ${el.scrollWidth}px, has ${el.clientWidth}px`);
          }
          for (const acts of Array.from(document.querySelectorAll<HTMLElement>('.acts-row, td.row-acts'))) {
            const row = acts.closest<HTMLElement>('.item, tr');
            const a = acts.getBoundingClientRect();
            const r = row?.getBoundingClientRect();
            if (r && a.width > 0 && (a.right > r.right + 1 || a.left < r.left - 1)) out.push(`row actions leave their row: ${Math.round(a.right)} > ${Math.round(r.right)}`);
          }
          // The footer row ("Files directly in this folder") lines up with the body rows: same swatch, same columns, text not cut.
          const foot = document.querySelector<HTMLElement>('tr.mat-mdc-footer-row:not(.foot-off)');
          const body = document.querySelector<HTMLElement>('tbody tr.mat-mdc-row');
          if (foot && body) {
            const left = (el: Element | null) => Math.round(el?.getBoundingClientRect().left ?? -1);
            const right = (el: Element | null) => Math.round(el?.getBoundingClientRect().right ?? -1);
            if (left(foot.querySelector('.swatch')) !== left(body.querySelector('.swatch'))) out.push('footer swatch is not under the body swatches');
            const cells = (tr: HTMLElement) => Array.from(tr.querySelectorAll('td'));
            for (const i of [1, 2, 4]) {
              if (right(cells(foot)[i] ?? null) !== right(cells(body)[i] ?? null)) out.push(`footer column ${i} is not aligned with the body rows`);
            }
            const txt = foot.querySelector<HTMLElement>('.txt');
            const cell = foot.querySelector('td');
            if (txt && cell && (txt.scrollWidth > txt.clientWidth + 1 || right(txt) > right(cell) - 8)) out.push(`footer text is cut by its cell: text ${txt.scrollWidth}/${txt.clientWidth}, text right ${right(txt)}, cell right ${right(cell)}`);
          }
          if (document.documentElement.scrollWidth > window.innerWidth) out.push(`the page scrolls sideways: ${document.documentElement.scrollWidth} > ${window.innerWidth}`);
          return out;
        });
      expect(await clipped()).toEqual([]);
      if (width === 900 || width === 1280) await page.screenshot({ path: process.env['DISK_MAP_SHOTS'] ? `${process.env['DISK_MAP_SHOTS']}/disk-map-chart-${width}.png` : info.outputPath(`chart-${width}.png`) });
      await page.getByRole('radio', { name: 'Table' }).click();
      await expect(page.getByRole('table', { name: /Contents of/ })).toBeVisible();
      expect(await clipped()).toEqual([]);
      if (width === 900 || width === 1280) await page.screenshot({ path: process.env['DISK_MAP_SHOTS'] ? `${process.env['DISK_MAP_SHOTS']}/disk-map-table-${width}.png` : info.outputPath(`table-${width}.png`) });
    });
  }
});
