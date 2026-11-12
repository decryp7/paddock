import { expect, test } from '@decryp7/paddock-e2e';

/** The IP lists, the connections map and the firewall against the mock backend (chromium). */
const url = (hash: string) => `/?latency=0#/${hash}`;

test.use({ reducedMotion: 'reduce' });

test.describe('IP lists', () => {
  test('adds a typed list, previews and applies "Remove duplicates", subscribes by address and removes a list', async ({ page }) => {
    await page.goto(url('lists'));
    await expect(page.getByRole('heading', { level: 1, name: 'IP lists' })).toBeVisible();
    await expect(page.getByTestId('list-list-1')).toContainText('Work blocklist');
    await expect(page.getByTestId('first-run-note')).toContainText('DB-IP');

    await page.getByLabel('New list name').fill('Messy');
    await page.getByLabel('New list entries').fill('8.8.8.8\n8.8.0.0/16\n8.8.8.8\nx.example\ny.x.example');
    await page.getByTestId('add-list').click();
    const messy = page.getByTestId('list-list-2');
    await expect(messy).toContainText('Messy');

    await messy.getByTestId('dedupe').click();
    await expect(page.getByTestId('preview')).toContainText('appears twice');
    await expect(page.getByTestId('preview')).toContainText('is covered by');
    await page.getByTestId('apply-dedupe').click();
    await expect(page.getByTestId('preview')).toHaveCount(0);
    await expect(messy.locator('.counts')).toContainText('2 entries');

    await page.getByRole('radio', { name: 'Subscribe by address' }).check();
    await expect(page.getByTestId('terms')).toContainText('your responsibility');
    await page.getByLabel('New list name').fill('Feed');
    await page.getByLabel('List address').fill('http://lists.example/feed.txt');
    await page.getByTestId('add-list').click();
    await expect(page.getByTestId('edit-error')).toContainText('The address must start with https://');
    await page.getByLabel('List address').fill('https://lists.example/feed.txt');
    await page.getByTestId('add-list').click();
    await expect(page.getByTestId('list-list-3')).toContainText('refreshed every 24 h');

    await messy.getByRole('button', { name: 'Remove', exact: true }).click();
    await page.getByTestId('confirm-remove').click();
    await expect(page.getByTestId('list-list-2')).toHaveCount(0);
  });

  test('turns a network switch off from the page', async ({ page }) => {
    await page.goto(url('lists'));
    const box = page.getByTestId('switch-own-location').getByRole('checkbox');
    await expect(box).toBeChecked();
    await box.uncheck();
    await expect(box).not.toBeChecked();
    await expect(page.getByTestId('switch-databases').getByRole('checkbox')).toBeChecked();
  });
});

test.describe('connections map', () => {
  test('draws the map with arcs from the home country, a table alternative and the credit', async ({ page }) => {
    await page.goto(url('map'));
    await expect(page.getByRole('heading', { level: 1, name: 'Connections map' })).toBeVisible();
    await expect(page.getByTestId('world-map')).toBeVisible();
    await expect(page.locator('path.arc')).toHaveCount(5);
    await expect(page.getByTestId('home-source')).toContainText('public IP');
    await expect(page.getByTestId('credit')).toContainText('DB-IP');

    const us = page.getByRole('button', { name: /^United States: 54 connections/ });
    await us.focus();
    await expect(us).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/#\/network$/);
    await expect(page.getByTestId('country-filter')).toContainText('US');
    await page.getByRole('button', { name: 'Show all countries' }).click();
    await expect(page.getByTestId('country-filter')).toHaveCount(0);

    await page.goto(url('map'));
    await page.getByRole('radio', { name: 'Table' }).click();
    await expect(page.getByTestId('country-table')).toBeVisible();
    await page.getByRole('radio', { name: 'Map' }).click();
    await expect(page.getByTestId('world-map')).toBeVisible();
  });

  test('has no animation under reduced motion', async ({ page }) => {
    await page.goto(url('map'));
    await expect(page.getByTestId('world-map')).toBeVisible();
    expect(await page.locator('svg animate, svg animateMotion, svg animateTransform').count()).toBe(0);
  });
});

test.describe('firewall', () => {
  test('shows the sources read-only and the one that needs the helper', async ({ page }) => {
    await page.goto(url('firewall'));
    await expect(page.getByRole('heading', { level: 1, name: 'Firewall' })).toBeVisible();
    await expect(page.locator('[data-source="applicationFirewall"]')).toContainText('Stealth mode: On');
    await expect(page.locator('[data-source="pf"]').getByTestId('reading-source-needs')).toContainText('only by an administrator');
    await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible();
  });
});

test.describe('firewall rule table at the smallest window (900 px)', () => {
  test.use({ viewport: { width: 900, height: 800 } });
  const PROGRAM = '/Applications/Example Sync.app/Contents/Library/LoginItems/Example Sync Background Service.app/Contents/MacOS/Example Sync Background Service';

  test('a long program path is cut in the middle: the file name stays, the full path is the tooltip, the cell is reachable by keyboard', async ({ page }) => {
    await page.goto('/?latency=0#/firewall'); // the mock is a Mac
    const path = page.locator('[data-source="applicationFirewall"] [data-testid="path-text"]').first();
    await expect(path).toBeVisible();
    await expect(path.locator('.pd-shown'), 'the end of the path (the program and its extension) stays').toContainText(/Service$/);
    await expect(path.locator('.pd-shown')).toContainText('…');
    await expect(path, 'a cut path is a keyboard stop').toHaveAttribute('tabindex', '0');
    await path.hover();
    const tip = page.locator('.mat-mdc-tooltip');
    await expect(tip).toBeVisible();
    // The tooltip's text is the program, with a zero-width break hint after each separator.
    expect((await tip.textContent())?.replaceAll('\u200b', '').trim(), 'the tooltip is exactly the rule\'s program').toBe(PROGRAM);
    const cut = await tip.locator('.mdc-tooltip__surface').evaluate((s) => s.scrollHeight - s.clientHeight);
    expect(cut, 'the tooltip shows the whole path (the end is not cut by its height)').toBeLessThanOrEqual(1);
  });

  test('the full path shows when the cut path is reached by the keyboard', async ({ page }) => {
    await page.goto('/?latency=0#/firewall');
    const path = page.locator('[data-source="applicationFirewall"] [data-testid="path-text"]').first();
    await expect(path).toBeVisible();
    for (let i = 0; i < 60; i++) {
      if (await path.evaluate((el) => el === document.activeElement)) break;
      await page.keyboard.press('Tab');
    }
    await expect(path, 'Tab reaches the cut path').toBeFocused();
    await expect(page.locator('.mat-mdc-tooltip')).toBeVisible();
    expect((await page.locator('.mat-mdc-tooltip').textContent())?.replaceAll('\u200b', '').trim()).toBe(PROGRAM);
  });

  test('the direction and the network are readable at 900 px (under the program, as the columns drop out)', async ({ page }) => {
    await page.goto('/?latency=0#/firewall');
    const rows = page.locator('[data-source="applicationFirewall"] pd-table');
    await expect(rows.getByText('Direction', { exact: true }), 'the Direction column is not drawn at 900 px').toHaveCount(0);
    await expect(rows).toContainText('Outgoing');
    await expect(rows).toContainText('All networks');
  });

  test('the rule name wraps to at most two lines', async ({ page }) => {
    await page.goto('/?latency=0#/firewall'); // the mock is a Mac
    const name = page.locator('[data-source="applicationFirewall"]').getByText('Example Sync helper (all networks)');
    await expect(name).toBeVisible();
    const lines = await name.evaluate((el) => {
      const cell = el.closest('[role="rowheader"], [role="gridcell"], th, td, mat-cell') ?? el;
      const lh = parseFloat(getComputedStyle(cell).lineHeight) || 16;
      return Math.round(cell.getBoundingClientRect().height / lh);
    });
    expect(lines, 'lines of the name cell').toBeLessThanOrEqual(2);
  });
});

test.describe('connections map legend, narrow window', () => {
  test.use({ viewport: { width: 400, height: 900 } });
  test('the five steps wrap as one block below the lead text and fit the legend', async ({ page }) => {
    await page.goto(url('map'));
    await page.getByTestId('world-map').waitFor();
    const r = await page.getByTestId('legend').evaluate((legend) => {
      const box = (el: Element) => el.getBoundingClientRect();
      const steps = [...legend.querySelectorAll('.key[data-step]')].map(box);
      const lead = box(legend.querySelector('.lead')!);
      return { tops: steps.map((b) => Math.round(b.top)), lead: { bottom: lead.bottom }, right: Math.max(...steps.map((b) => b.right)), left: Math.min(...steps.map((b) => b.left)), legend: { left: box(legend).left, right: box(legend).right } };
    });
    expect(new Set(r.tops).size, 'all five steps on one line').toBe(1);
    expect(r.tops[0]!, 'the steps sit below the lead text').toBeGreaterThanOrEqual(Math.floor(r.lead.bottom));
    expect(r.left).toBeGreaterThanOrEqual(r.legend.left - 0.5);
    expect(r.right, 'the steps fit the legend').toBeLessThanOrEqual(r.legend.right + 0.5);
  });
});

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`map contrast, ${scheme}`, () => {
    test.use({ colorScheme: scheme });
    test('the land is told from the sea, and the lowest legend band from the land, by at least 1.5 to 1', async ({ page }) => {
      await page.goto(url('map'));
      await page.getByTestId('world-map').waitFor();
      const ratios = await page.evaluate(() => {
        const rgb = (css: string): [number, number, number] => {
          const c = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!;
          c.fillStyle = '#000';
          c.fillStyle = css;
          c.fillRect(0, 0, 1, 1);
          const d = c.getImageData(0, 0, 1, 1).data;
          return [d[0]!, d[1]!, d[2]!];
        };
        const lum = ([r, g, b]: [number, number, number]) => {
          const f = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
          return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
        };
        const ratio = (a: string, b: string) => {
          const [x, y] = [lum(rgb(a)), lum(rgb(b))];
          return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
        };
        const sea = getComputedStyle(document.querySelector('path.sea')!).fill;
        const plain = getComputedStyle(document.querySelector('path.land[data-step="-1"]')!).fill;
        const band0 = getComputedStyle(document.querySelector('path.land[data-step="0"]')!).fill;
        return { seaVsLand: ratio(sea, plain), band0VsLand: ratio(band0, plain), band0VsSea: ratio(band0, sea) };
      });
      expect(ratios.seaVsLand).toBeGreaterThanOrEqual(1.5);
      expect(ratios.band0VsLand).toBeGreaterThanOrEqual(1.5);
      expect(ratios.band0VsSea).toBeGreaterThanOrEqual(1.5);
    });

    test('neighbouring legend steps differ in lightness (1.3 to 1 or more), and each key fill is the map\'s fill for that step', async ({ page }) => {
      await page.goto(url('map'));
      await page.getByTestId('world-map').waitFor();
      const r = await page.evaluate(() => {
        const rgb = (css: string): [number, number, number] => {
          const c = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!;
          c.fillStyle = '#000';
          c.fillStyle = css;
          c.fillRect(0, 0, 1, 1);
          const d = c.getImageData(0, 0, 1, 1).data;
          return [d[0]!, d[1]!, d[2]!];
        };
        const lum = ([r, g, b]: [number, number, number]) => {
          const f = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
          return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
        };
        const keys = [0, 1, 2, 3, 4].map((i) => getComputedStyle(document.querySelector(`[data-testid="legend"] svg.swatch[data-step="${i}"] rect`)!).fill);
        // A land path in every step, so the map's own fill is compared (the mock has countries in only some steps).
        const probe = document.querySelector('path.land[data-step="-1"]')!;
        const land = [0, 1, 2, 3, 4].map((i) => {
          probe.setAttribute('data-step', String(i));
          return getComputedStyle(probe).fill;
        });
        probe.setAttribute('data-step', '-1');
        const ratios = keys.slice(1).map((k, i) => {
          const [x, y] = [lum(rgb(k)), lum(rgb(keys[i]!))];
          return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
        });
        return { keys, land, ratios, plain: getComputedStyle(probe).fill };
      });
      expect(r.keys).toEqual(r.land);
      // The ramp is defined for this app's accent: no step falls back to the plain land colour.
      expect(r.keys).not.toContain(r.plain);
      for (const [i, ratio] of r.ratios.entries()) expect(ratio, `step ${i + 1} vs ${i + 2}`).toBeGreaterThanOrEqual(1.3);
    });
  });
}
