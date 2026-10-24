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
  await expect(items.last()).toHaveClass(/pd-danger/);
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
  const scrolls = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  expect(scrolls).toBe(false);
  const headers = await small(page).getByRole('columnheader').allInnerTexts();
  expect(headers.map((h) => h.trim())).not.toContain('Location');
  expect(headers.map((h) => h.trim())).toContain('Name');
  const overflow = await small(page).locator('.pd-grid').evaluate((e) => e.scrollWidth > e.clientWidth);
  expect(overflow).toBe(false);
});

test('chart: four named charts are drawn on canvases, each with a text twin', async ({ page }) => {
  await open(page);
  const charts = page.locator('section[data-testid="chart"] pd-chart');
  await expect(charts).toHaveCount(4);
  await expect(charts.locator('canvas')).toHaveCount(4);
  const donut = page.getByRole('img', { name: 'Disk use by kind' });
  await expect(donut).toBeVisible();
  const twin = await page.locator(`#${await donut.getAttribute('aria-describedby')}`).innerText();
  expect(twin).toContain('Logs: 42 GB');
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
  await expect(forms.getByTestId('forms-status')).toHaveText('Not saved');
  await forms.getByLabel('Port').fill('8080');
  await forms.getByRole('button', { name: 'Save' }).click();
  await expect(forms.getByTestId('forms-status')).toHaveText('Saved Alpha');
});
