import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';

// <pd-view> (plan X51, ADR 0002) in the gallery: every node kind drawn from one sample document, the actions it sends, patches,
// the inline errors, axe in both themes, and no horizontal scroll of the page at 320 px (wide tables and drawings scroll in their
// own region).

const THEMES = ['dark', 'light'] as const;

async function open(page: Page, theme = 'dark', width = 1280): Promise<Locator> {
  await page.setViewportSize({ width, height: 900 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  const box = page.locator('[data-testid="view-box"]');
  await box.locator('pd-view article').waitFor();
  // The five charts have drawn.
  await expect(box.locator('pd-chart canvas')).toHaveCount(5);
  return box;
}
const log = (page: Page) => page.getByTestId('view-log');

for (const theme of THEMES) {
  test(`axe finds no violations in the view (${theme})`, async ({ page }) => {
    await open(page, theme);
    const results = await new AxeBuilder({ page }).include('[data-testid="view-box"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });
}

test('draws a node of every kind and one "update the app" for the unknown one', async ({ page }) => {
  const box = await open(page);
  for (const kind of ['stack', 'text', 'table', 'tree', 'graph', 'diagram', 'form', 'reviewList', 'matrix', 'chart', 'badge', 'progress', 'empty', 'needsAi', 'link']) {
    await expect(box.locator(`[data-kind="${kind}"]`).first(), kind).toBeVisible();
  }
  await expect(box.getByTestId('view-unknown')).toHaveCount(1);
  await expect(box.getByTestId('view-unknown')).toHaveText('Update the app to view this');
});

test('table: sort and paging send their actions with the host keys', async ({ page }) => {
  const box = await open(page);
  await box.locator('[data-node-id="t1"]').getByRole('button', { name: 'Sort by State, ascending' }).click();
  await expect(log(page)).toContainText('sort {"column":"state","descending":false}');
  await box.getByTestId('view-table-next').click();
  await expect(log(page)).toContainText('page {"page":2}');
  await box.locator('[data-node-id="t1"]').getByRole('button', { name: 'Open: Integration tests' }).click();
  await expect(log(page)).toContainText('open-check {"row":1}');
});

test('tree: a lazy node asks for its children, one with children opens at once', async ({ page }) => {
  const box = await open(page);
  const tree = box.locator('[data-node-id="tree"]');
  await tree.getByRole('button', { name: 'Expand: crates' }).click();
  await expect(tree.getByRole('treeitem')).toHaveCount(3);
  await tree.getByRole('button', { name: 'Expand: ui' }).click();
  await expect(log(page)).toContainText('expand {"node":"ui"}');
});

test('form: an invalid form sends nothing; a valid one asks the host, then sends its values', async ({ page }) => {
  const box = await open(page);
  const form = box.locator('[data-node-id="form"]');
  await form.getByLabel('Release name').fill('');
  await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
  await expect(form.getByTestId('field-error').first()).toHaveText('This is required.');
  await expect(log(page)).toContainText('Last action: none');
  await form.getByLabel('Release name').fill('v2');
  await form.getByRole('button', { name: 'Output folder: Choose a folder…' }).click();
  await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
  await expect(page.getByTestId('view-confirm')).toContainText('Publish release v1.4?');
  await page.getByTestId('view-confirm-no').click();
  await expect(log(page)).toContainText('Last action: none');
  await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
  await page.getByTestId('view-confirm-yes').click();
  await expect(log(page)).toContainText('publish {"values":{"name":"v2"');
  await expect(log(page)).toContainText('"out":"grant-folder-1"');
});

test('review: a state that needs a reason asks for it before sending', async ({ page }) => {
  const box = await open(page);
  const review = box.locator('[data-node-id="review"]');
  await review.getByRole('button', { name: /^Suppress: / }).click();
  await review.getByTestId('view-reason-send').click();
  await expect(review.getByTestId('field-error')).toHaveText('This is required.');
  await review.getByTestId('view-reason').fill('Known issue');
  await review.getByTestId('view-reason-send').click();
  await expect(log(page)).toContainText('suppress {"finding":"f3","reason":"Known issue"}');
});

test('links: a web address is confirmed by the host before it opens', async ({ page }) => {
  const box = await open(page);
  await box.locator('[data-node-id="l2"]').getByRole('button').click();
  await expect(page.getByTestId('view-confirm')).toContainText('https://example.com/guide');
  await page.getByTestId('view-confirm-yes').click();
  await expect(log(page)).toContainText('Opened: https://example.com/guide');
});

test('patches: a replace-by-id updates one node; a stale one is an error with Retry and the view stays', async ({ page }) => {
  const box = await open(page);
  await page.getByTestId('view-patch').click();
  await expect(box.locator('[data-node-id="cap"]')).toContainText('Updated at revision 2');
  await page.getByTestId('view-stale').click();
  await expect(box.getByTestId('view-error')).toContainText('out of date');
  await expect(box.locator('[data-node-id="cap"]')).toContainText('Updated at revision 2');
  await box.getByTestId('view-retry').click();
  await expect(box.getByTestId('view-error')).toHaveCount(0);
  await expect(box.locator('[data-node-id="cap"]')).toContainText('Updated a minute ago');
});

test('an invalid document is an inline error with Retry and nothing of it is drawn', async ({ page }) => {
  const box = await open(page);
  await page.getByTestId('view-bad').click();
  await expect(box.getByTestId('view-error')).toContainText('Update the app');
  await expect(box.locator('article')).toHaveCount(0);
  await box.getByTestId('view-retry').click();
  await expect(box.locator('article')).toBeVisible();
});

for (const theme of THEMES) {
  test(`no horizontal scroll at 320 px; wide parts scroll inside their own region (${theme})`, async ({ page }) => {
    const box = await open(page, theme, 320);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false);
    expect(await box.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(false);
    // Nothing inside the view is wider than the view except the regions that scroll themselves.
    const wide = await box.evaluate((root) => {
      const limit = root.getBoundingClientRect().right + 1;
      const out: string[] = [];
      for (const el of Array.from(root.querySelectorAll<HTMLElement>('*'))) {
        if (el.closest('[role=region]') || el.closest('svg') || el.closest('canvas') || el.tagName === 'CANVAS') continue;
        if (el.getBoundingClientRect().right > limit) out.push(`${el.tagName.toLowerCase()}.${el.className}`);
      }
      return out;
    });
    expect(wide).toEqual([]);
    // The big graph scrolls in its own region.
    const region = box.locator('[data-node-id="g1"] [role=region]');
    expect(await region.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
  });
}
