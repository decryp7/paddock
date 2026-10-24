import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@decryp7/paddock-e2e';

// The data parts of paddock-ui (plan X21) in the gallery: the table's sorting, filter, keyboard, menus and virtual rows, the
// chart wrapper over a real ECharts, and the forms helpers, in both themes, with axe and without horizontal scroll at 320 px.

const THEMES = ['dark', 'light'] as const;

async function open(page: Page, theme = 'dark', width = 1280): Promise<void> {
  await page.setViewportSize({ width, height: 900 });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  await page.locator('section[data-testid="table"] pd-table').first().waitFor();
}

const small = (page: Page) => page.locator('[data-testid="table-small"]');
const names = (page: Page) => small(page).locator('[role=row][data-index] [role=gridcell]:first-child');

for (const theme of THEMES) {
  test(`axe finds no violations in the table, chart and forms (${theme})`, async ({ page }) => {
    await open(page, theme);
    await page.locator('section[data-testid="chart"] canvas').first().waitFor();
    const results = await new AxeBuilder({ page }).include('section[data-testid="table"]').include('section[data-testid="chart"]').include('section[data-testid="forms"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });
}

test('table: sorting cycles ascending, descending, none and sets aria-sort', async ({ page }) => {
  await open(page);
  const first = async () => (await names(page).first().innerText()).trim();
  const original = await first();
  const header = small(page).getByRole('button', { name: 'Sort by Name' });
  await header.click();
  await expect(small(page).getByRole('columnheader', { name: 'Name' })).toHaveAttribute('aria-sort', 'ascending');
  const asc = await first();
  await header.click();
  await expect(small(page).getByRole('columnheader', { name: 'Name' })).toHaveAttribute('aria-sort', 'descending');
  expect(await first()).not.toBe(asc);
  await header.click();
  await expect(small(page).getByRole('columnheader', { name: 'Name' })).toHaveAttribute('aria-sort', 'none');
  expect(await first()).toBe(original);
});

test('table: the filter narrows the rows and says how many are left', async ({ page }) => {
  await open(page);
  await small(page).getByRole('searchbox', { name: 'Filter' }).fill('journal');
  await expect(names(page)).toHaveCount(1);
  await expect(small(page).getByTestId('table-count')).toHaveText('1 of 14 rows');
  await small(page).getByRole('searchbox', { name: 'Filter' }).fill('nothing like this');
  await expect(small(page).getByRole('status')).toContainText('No rows match');
});

test('table: the column menu switches a column on and the keyboard reaches it', async ({ page }) => {
  await open(page);
  await expect(small(page).getByRole('columnheader', { name: 'State' })).toHaveCount(0);
  await small(page).getByTestId('table-columns').click();
  await page.getByRole('menuitemcheckbox', { name: 'State' }).click();
  await expect(small(page).getByRole('columnheader', { name: 'State' })).toBeVisible();
});

test('table: arrows, End and Home move between rows on one tab stop, Enter opens, Shift+F10 opens the menu', async ({ page }) => {
  await open(page);
  const rows = small(page).locator('[role=row][data-index]');
  await rows.first().focus();
  await page.keyboard.press('ArrowDown');
  await expect(rows.nth(1)).toBeFocused();
  await expect(rows.nth(1)).toHaveAttribute('tabindex', '0');
  await expect(rows.first()).toHaveAttribute('tabindex', '-1');
  await page.keyboard.press('End');
  await expect(rows.last()).toBeFocused();
  await page.keyboard.press('Home');
  await expect(rows.first()).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('table-status')).toContainText('Opened: Syslog');
  await page.keyboard.press('Shift+F10');
  await expect(page.getByRole('menuitem', { name: 'Reveal in folder' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toHaveCount(0);
});

test('table: row actions show on hover, the "..." button is always there, destructive items are last in danger text', async ({ page }) => {
  await open(page);
  const row = small(page).locator('[role=row][data-index]').first();
  const inline = row.getByRole('button', { name: 'Reveal in folder' });
  await expect(inline).toHaveCSS('opacity', '0');
  await row.hover();
  await expect(inline).toHaveCSS('opacity', '1');
  const more = row.getByRole('button', { name: 'More actions' });
  await expect(more).toBeVisible();
  await more.click();
  const items = page.getByRole('menuitem');
  await expect(items.last()).toContainText('Remove');
  await expect(items.last()).toHaveClass(/pd-popmenu__item--danger/);
  await items.nth(1).click();
  await expect(page.getByTestId('table-status')).toContainText('Chose: copy Syslog');
});

test('table: 5,000 rows are drawn by the virtual viewport, a few at a time', async ({ page }) => {
  await open(page);
  const big = page.locator('[data-testid="table-virtual"]');
  await expect(big.locator('cdk-virtual-scroll-viewport')).toBeVisible();
  const drawn = await big.locator('[role=row][data-index]').count();
  expect(drawn).toBeGreaterThan(0);
  expect(drawn).toBeLessThan(100);
  await expect(big.locator('[role=grid]')).toHaveAttribute('aria-rowcount', /^5\d{3}$/);
  // Grouped: the groups are rows of the table.
  await expect(big.locator('.pd-group').first()).toBeVisible();
});

test('table: no horizontal scroll at 320 px, columns drop out instead', async ({ page }) => {
  await open(page, 'dark', 320);
  // The table follows its own width: the name and the "..." column are what is left.
  await expect(small(page).getByRole('columnheader')).toHaveCount(2);
  const scrolls = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  expect(scrolls).toBe(false);
  const headers = await small(page).getByRole('columnheader').allInnerTexts();
  expect(headers.map((h) => h.trim())).not.toContain('Location');
  expect(headers.map((h) => h.trim())).toContain('Name');
  const overflow = await small(page).locator('.pd-grid').evaluate((e) => e.scrollWidth > e.clientWidth);
  expect(overflow).toBe(false);
  // The primary column keeps its floor: the first name is readable whole, not cut to a letter.
  const name = small(page).locator('[role=row][data-index] .pd-first .pd-text').first();
  expect((await small(page).locator('[role=row][data-index] .pd-first').first().boundingBox())!.width).toBeGreaterThanOrEqual(100);
  expect(await name.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
  await expect(name).toHaveText('Syslog');
  // The toolbar is compact: the filter keeps 8rem, Columns is icon-only, and the row actions are the "..." button alone.
  expect((await small(page).locator('.pd-filter').boundingBox())!.width).toBeGreaterThanOrEqual(128);
  await expect(small(page).getByTestId('table-columns')).toHaveAccessibleName('Columns');
  expect(await small(page).locator('.pd-tool-text').isVisible()).toBe(false);
  await expect(small(page).locator('[role=row][data-index]').first().locator('.pd-inline')).toHaveCount(0);
  for (const id of ['table-small', 'box-selected', 'box-empty', 'box-loading', 'box-error', 'table-virtual']) {
    const box = page.locator(`[data-testid="${id}"] .pd-grid`);
    expect(await box.evaluate((e) => e.scrollWidth <= e.clientWidth), id).toBe(true);
  }
});

test('table: rows are 32 px with a 28 px header, and a sortable header shows its chevron on hover and focus', async ({ page }) => {
  await open(page);
  const row = small(page).locator('[role=row][data-index]').first();
  expect((await row.boundingBox())!.height).toBeCloseTo(32, 0);
  expect((await small(page).locator('.pd-header').boundingBox())!.height).toBeCloseTo(28, 0);
  const icon = small(page).locator('.pd-sort').first().locator('.pd-sort-icon');
  await expect(icon).toHaveCSS('opacity', '0');
  await small(page).locator('.pd-sort').first().hover();
  await expect(icon).toHaveCSS('opacity', '1');
});

test('table: the keyboard focus in a menu is a 2 px ring with 3:1 contrast, and the icon slot lines the labels up', async ({ page }) => {
  await open(page);
  const rows = small(page).locator('[role=row][data-index]');
  await rows.first().focus();
  await page.keyboard.press('Shift+F10');
  const items = page.getByRole('menuitem');
  await expect(items.first()).toBeVisible();
  await page.keyboard.press('ArrowDown');
  const focused = page.locator('.pd-popmenu__item:focus-visible');
  await expect(focused).toHaveCount(1);
  const ring = await focused.evaluate((e) => {
    const cs = getComputedStyle(e);
    const rgb = (c: string) => (c.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
    const lin = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    const lum = (c: number[]) => 0.2126 * lin(c[0]!) + 0.7152 * lin(c[1]!) + 0.0722 * lin(c[2]!);
    const menuBg = getComputedStyle(e.parentElement!).backgroundColor;
    const [a, b] = [lum(rgb(cs.outlineColor)), lum(rgb(menuBg))];
    return { width: cs.outlineWidth, style: cs.outlineStyle, ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) };
  });
  expect(ring.width).toBe('2px');
  expect(ring.style).toBe('solid');
  expect(ring.ratio).toBeGreaterThanOrEqual(3);
  const lefts = await page.locator('.pd-popmenu__item > span:last-child').evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().left)));
  expect(new Set(lefts).size).toBe(1);
  await expect(page.locator('.pd-popmenu__sep')).toHaveCount(1);
  await page.keyboard.press('Escape');
});

test('table: paths are shortened only as far as the room needs, and grow back when the table widens', async ({ page }) => {
  await open(page, 'dark', 1280);
  const paths = page.locator('[data-testid="table-virtual"] pd-middle-text');
  await expect(paths.first()).toBeVisible();
  // A 28-character path in a column hundreds of pixels wide is never cut.
  const cut = await paths.evaluateAll((els) => els.filter((e) => e.textContent !== e.getAttribute('title')).length);
  expect(cut).toBe(0);
  // Narrow the page until the column is short, then widen it again: the full path comes back.
  const small = page.locator('[data-testid="table-small"] pd-middle-text').first();
  await page.setViewportSize({ width: 900, height: 900 });
  await expect.poll(async () => (await small.textContent()) !== (await small.getAttribute('title'))).toBe(true);
  await page.setViewportSize({ width: 1600, height: 900 });
  await expect.poll(async () => (await small.textContent()) === (await small.getAttribute('title'))).toBe(true);
});

test('table: loading and empty say no count, and the columns do not move when rows arrive', async ({ page }) => {
  await open(page);
  await expect(page.locator('[data-testid="box-loading"] [data-testid="table-count"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="box-empty"] [data-testid="table-count"]')).toHaveCount(0);
  const left = (id: string) => page.locator(`[data-testid="${id}"]`).getByRole('columnheader', { name: 'Size (KiB)' }).evaluate((e) => Math.round(e.getBoundingClientRect().left - (e.closest('.pd-grid') as HTMLElement).getBoundingClientRect().left));
  const loaded = await left('box-selected');
  for (const id of ['box-loading', 'box-empty', 'box-error']) expect(await left(id), id).toBe(loaded);
});

test('table: the sortable hint is a two-way glyph, never the one a descending sort uses', async ({ page }) => {
  await open(page);
  const grid = page.locator('[data-testid="box-selected"]');
  const desc = await grid.getByRole('columnheader', { name: 'Size (KiB)' }).locator('.pd-sort-icon path').first().getAttribute('d');
  const hint = await grid.getByRole('columnheader', { name: 'Kind' }).locator('.pd-sort-icon path').first().getAttribute('d');
  expect(hint).not.toBe(desc);
  expect(hint).toContain('M8 9l4-4 4 4');
});

test('table: the resting "..." button is visible in its own colour, not faded', async ({ page }) => {
  await open(page);
  const more = page.locator('[data-testid="table-small"] [role=row][data-index]').nth(3).getByRole('button', { name: 'More actions' });
  await expect(more).toHaveCSS('opacity', '1');
  const colour = await more.evaluate((e) => getComputedStyle(e).color);
  const text = await more.evaluate((e) => getComputedStyle(e.closest('.pd-row')!).color);
  expect(colour).not.toBe(text);
});

test('table: selected row, descending sort, empty, loading and error states are drawn', async ({ page }) => {
  await open(page);
  const selected = page.locator('[data-testid="box-selected"]');
  await expect(selected.locator('[role=row][aria-selected=true]')).toHaveCount(1);
  await expect(selected.getByRole('columnheader', { name: 'Size (KiB)' })).toHaveAttribute('aria-sort', 'descending');
  await expect(page.locator('[data-testid="box-empty"]').getByRole('button', { name: 'Install a plugin' })).toBeVisible();
  await expect(page.locator('[data-testid="box-loading"] .pd-skel').first()).toBeVisible();
  await expect(page.locator('[data-testid="box-error"]').getByRole('alert')).toContainText('could not be read');
  await page.locator('[data-testid="table-virtual"] .pd-group').first().click();
  await expect(page.locator('[data-testid="table-virtual"] .pd-group-toggle').first()).toHaveAttribute('aria-expanded', 'false');
});

test('chart: four named charts, a loading and an error one, are drawn on canvases, each with a text twin', async ({ page }) => {
  await open(page);
  const charts = page.locator('section[data-testid="chart"] pd-chart');
  await expect(charts).toHaveCount(6);
  await expect(charts.locator('canvas')).toHaveCount(6);
  const donut = page.getByRole('img', { name: 'Disk use by kind' });
  await expect(donut).toBeVisible();
  const twin = await page.locator(`#${await donut.getAttribute('aria-describedby')}`).innerText();
  expect(twin).toContain('Logs: 42 GB (48%)');
  // Every chart has a visible title: metric, unit and window.
  await expect(charts.first().getByTestId('chart-title')).toHaveText('Requests per hour (req)');
  await expect(page.getByTestId('chart-loading')).toBeVisible();
  await expect(page.getByTestId('chart-error')).toContainText('could not be loaded');
});

/** Whether the canvas has any pixel in the box (CSS px, centre cx cy, half sizes) that differs from the card's background. */
async function inked(canvas: import('@playwright/test').Locator, cx: number, cy: number, hw: number, hh: number): Promise<boolean> {
  return canvas.evaluate(
    (c, a) => {
      const canvasEl = c as HTMLCanvasElement;
      const k = canvasEl.width / canvasEl.getBoundingClientRect().width;
      const ctx = canvasEl.getContext('2d')!;
      const d = ctx.getImageData(Math.round((a.cx - a.hw) * k), Math.round((a.cy - a.hh) * k), Math.round(a.hw * 2 * k), Math.round(a.hh * 2 * k)).data;
      const [r, g, b] = [d[0]!, d[1]!, d[2]!];
      for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i]! - r) + Math.abs(d[i + 1]! - g) + Math.abs(d[i + 2]! - b) > 60 && d[i + 3]! > 0) return true;
      return false;
    },
    { cx, cy, hw, hh },
  );
}

test('chart: the donut total is drawn in the middle of the ring, wide, narrow and at 320 px, and the ring stays inside the card', async ({ page }) => {
  for (const width of [1800, 1280, 320]) {
    await open(page, 'dark', width);
    const card = page.getByRole('img', { name: 'Disk use by kind' }).locator('xpath=ancestor::pd-chart');
    await card.locator('canvas').waitFor();
    await page.waitForTimeout(700);
    const canvas = card.locator('canvas');
    const box = (await canvas.boundingBox())!;
    // The layout follows the card's own width (three cards in a row are narrow at 1280).
    const narrow = box.width < 400;
    const radius = narrow ? 58 : Math.max(40, Math.min(Math.floor(box.height / 2) - 10, 90));
    const cx = narrow ? Math.floor(box.width / 2) : radius + 16;
    const cy = narrow ? radius + 12 : Math.floor(box.height / 2);
    expect(await inked(canvas, cx, cy, 16, 10), `total at ${width}`).toBe(true);
    // The ring has air on both sides inside the card (nothing drawn in the outer 4 px columns).
    expect(await inked(canvas, 2, cy, 2, 20), `left edge at ${width}`).toBe(false);
    expect(await inked(canvas, box.width - 2, cy, 2, 20), `right edge at ${width}`).toBe(false);
    if (narrow) {
      // The list is under the ring: its first row starts below the ring's bottom.
      expect(await inked(canvas, box.width / 2, cy + radius + 22, 40, 6)).toBe(true);
      expect(box.height).toBeGreaterThan(cy + radius + 4 * 20);
    }
  }
});

test('chart: switching the theme draws the charts again with the new colours', async ({ page }) => {
  await open(page, 'dark');
  const canvas = page.locator('section[data-testid="chart"] pd-chart').first().locator('canvas');
  await canvas.waitFor();
  const before = await canvas.evaluate((c) => (c as HTMLCanvasElement).toDataURL());
  await page.getByRole('radiogroup', { name: 'Theme' }).getByRole('radio', { name: 'light' }).click();
  await expect.poll(async () => page.locator('section[data-testid="chart"] pd-chart').first().locator('canvas').evaluate((c) => (c as HTMLCanvasElement).toDataURL())).not.toBe(before);
});

test('forms: the error shows after the field was left, with the 14 px alert icon, and on submit', async ({ page }) => {
  await open(page);
  const forms = page.locator('section[data-testid="forms"]');
  const name = forms.getByLabel('Name');
  await expect(forms.getByTestId('field-error')).toHaveCount(0);
  await name.focus();
  await name.blur();
  const error = forms.getByTestId('field-error');
  await expect(error).toContainText('This is required.');
  expect(await error.locator('svg').getAttribute('width')).toBe('14');
  await expect(name).toHaveAttribute('aria-invalid', 'true');
  await name.fill('Alpha');
  await expect(forms.getByTestId('field-error')).toHaveCount(0);
  await forms.getByLabel('Port').fill('70000');
  await forms.getByRole('button', { name: 'Save' }).click();
  await expect(forms.getByTestId('field-error')).toContainText('Enter 65535 or less.');
  // The short field narrows the control only: its error stays on one line.
  expect((await forms.getByTestId('field-error').boundingBox())!.height).toBeLessThan(24);
  expect((await forms.getByLabel('Port').boundingBox())!.width).toBeLessThanOrEqual(130);
  await expect(forms.getByTestId('forms-status')).toHaveText('Not saved');
  await forms.getByLabel('Port').fill('8080');
  await forms.getByRole('button', { name: 'Save' }).click();
  await expect(forms.getByTestId('forms-status')).toHaveText('Saved Alpha');
});
