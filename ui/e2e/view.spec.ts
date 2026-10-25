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
  await expect(box.getByTestId('view-unknown')).toContainText('Update the app to view this');
});

test('needs local AI: the only action is the app\'s Open AI settings, the plugin\'s setup is never sent', async ({ page }) => {
  const box = await open(page);
  const card = box.getByTestId('view-needs-ai');
  await expect(card).toContainText('Pitwall is not running.');
  await expect(card.getByRole('button')).toHaveCount(1);
  await expect(card).not.toContainText('Set up Pitwall');
  await card.getByRole('button', { name: 'Open AI settings' }).click();
  await expect(log(page)).toContainText('AI settings opened: 1');
  await expect(log(page)).not.toContainText('setup-ai');
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
  await expect(box.getByTestId('view-retry')).toHaveText('Reload');
  await expect(box.locator('[data-node-id="cap"]')).toContainText('Updated at revision 2');
  await box.getByTestId('view-retry').click();
  await expect(box.getByTestId('view-error')).toHaveCount(0);
  await expect(box.locator('[data-node-id="cap"]')).toContainText('Updated a minute ago');
});

test('an invalid document is an inline error with Retry and nothing of it is drawn', async ({ page }) => {
  const box = await open(page);
  await page.getByTestId('view-bad').click();
  await expect(box.getByTestId('view-error')).toContainText('Update the app');
  await expect(box.getByTestId('view-error')).toContainText('Release checks sent a view this app cannot show');
  await expect(box.getByTestId('view-retry')).toHaveText('Retry');
  await expect(box.locator('article')).toHaveCount(0);
  await box.getByTestId('view-retry').click();
  await expect(box.locator('article')).toBeVisible();
});

for (const theme of THEMES) {
  test(`no horizontal scroll at 320 px; wide parts scroll inside their own region (${theme})`, async ({ page }) => {
    const box = await open(page, theme, 320);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false);
    expect(await box.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(false);
    // Nothing inside the view is wider than the view except the regions that scroll themselves (the drawings and tables, the review filter).
    const wide = await box.evaluate((root) => {
      const limit = root.getBoundingClientRect().right + 1;
      const out: string[] = [];
      for (const el of Array.from(root.querySelectorAll<HTMLElement>('*'))) {
        if (el.closest('[role=region]') || el.closest('.pd-view-review__filter') || el.closest('svg') || el.closest('canvas') || el.tagName === 'CANVAS') continue;
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

// ---- Review 1 fixes (docs/review/x51-view/FIXES.md): what only a real layout can show.

const overlaps = (a: DOMRect, b: DOMRect): boolean => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

test('P1-1: the radial graph shows all its boxes, apart, in a canvas that fits them', async ({ page }) => {
  const box = await open(page);
  const g = box.locator('[data-node-id="g2"]');
  const boxes = await g.locator('g.pd-view-scene__node rect').evaluateAll((rs) => rs.map((r) => r.getBoundingClientRect().toJSON() as DOMRect));
  expect(boxes).toHaveLength(4);
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) expect(overlaps(boxes[i]!, boxes[j]!), `${i}/${j}`).toBe(false);
  const svg = await g.locator('svg.pd-view-scene__svg').evaluate((s) => s.getBoundingClientRect().toJSON() as DOMRect);
  const right = Math.max(...boxes.map((b) => b.right));
  const bottom = Math.max(...boxes.map((b) => b.bottom));
  // The canvas is the boxes and the padding, not a 1,000 px field around four boxes.
  expect(svg.right - right).toBeLessThanOrEqual(20);
  expect(svg.bottom - bottom).toBeLessThanOrEqual(20);
  expect(svg.width).toBeLessThan(700);
});

test('P1-2: every toned box of the graph carries a glyph and the list names the status', async ({ page }) => {
  const box = await open(page);
  const g = box.locator('[data-node-id="g1"]');
  await expect(g.locator('g.pd-view-scene__node')).toHaveCount(5);
  await expect(g.locator('.pd-view-scene__mark')).toHaveCount(4);
  await g.locator('details summary').click();
  await expect(g.getByTestId('view-scene-nodes')).toContainText('Publish — Problem');
});

test('P2-1: a row of badges packs at the gap instead of spreading', async ({ page }) => {
  const box = await open(page);
  const rects = await box.locator('[data-node-id="badges"] .pd-view-badge').evaluateAll((bs) => bs.map((b) => b.getBoundingClientRect().toJSON() as DOMRect));
  expect(rects).toHaveLength(5);
  for (const r of rects) expect(Math.abs(r.top - rects[0]!.top)).toBeLessThan(2);
  for (let i = 1; i < rects.length; i++) expect(rects[i]!.left - rects[i - 1]!.right).toBeLessThanOrEqual(9);
});

test('P2-2 and P2-3: words break only when they must, and a heading is smaller than the view title', async ({ page }) => {
  const box = await open(page);
  expect(await box.locator('[data-node-id="lead"] p').evaluate((e) => getComputedStyle(e).overflowWrap)).toBe('break-word');
  const size = (sel: string) => box.locator(sel).first().evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
  expect(await size('[data-node-id="h"] p')).toBeLessThan(await size('[data-testid=view-title]'));
  expect(await size('[data-node-id="h"] p')).toBeGreaterThan(await size('[data-node-id="lead"] p'));
});

test('P2-6: one filled accent button in the whole view', async ({ page }) => {
  const box = await open(page);
  await expect(box.locator('.pd-btn--primary')).toHaveCount(1);
  await expect(box.locator('[data-node-id="form"] .pd-btn--primary')).toHaveCount(1);
});

test('P2-5: loading is a skeleton and busy is a visible status', async ({ page }) => {
  const box = await open(page);
  await page.getByTestId('view-busy-demo').click();
  await expect(box.getByTestId('view-busy')).toContainText('Working…');
  await page.getByTestId('view-busy-demo').click();
  await expect(box.getByTestId('view-busy')).toHaveCount(0);
  await page.getByTestId('view-load-demo').click();
  await expect(box.getByTestId('view-loading')).toBeVisible();
  expect((await box.getByTestId('view-loading').boundingBox())!.height).toBeGreaterThan(100);
  await page.getByTestId('view-reload').click();
  await expect(box.locator('article')).toBeVisible();
});

test('P2-8 and P2-9: tree status colours the mark only; numbers sit on the right', async ({ page }) => {
  const box = await open(page);
  const tree = box.locator('[data-node-id="tree"]');
  await tree.getByRole('button', { name: 'Expand: crates' }).click();
  const colours = await tree.locator('[data-node="view"]').evaluate((row) => {
    const label = getComputedStyle(row.querySelector('.pd-view-tree__label')!).color;
    const mark = getComputedStyle(row.querySelector('pd-view-tone')!).color;
    return { label, mark, text: getComputedStyle(row).color };
  });
  expect(colours.label).toBe(colours.text);
  expect(colours.mark).not.toBe(colours.text);
  const t = box.locator('[data-node-id="t1"]');
  expect(await t.locator('tbody tr').first().locator('td').nth(2).evaluate((e) => getComputedStyle(e).textAlign)).toBe('end');
  expect(await t.locator('th').nth(2).evaluate((e) => getComputedStyle(e).textAlign)).toBe('end');
  await expect(t.locator('th').nth(2)).toContainText('Time (ms)');
  expect(await t.locator('tbody tr').first().locator('td').nth(1).evaluate((e) => getComputedStyle(e).whiteSpace)).toBe('nowrap');
});

test('P2-10: the form shows the name the host gave the chosen folder and secret', async ({ page }) => {
  const box = await open(page);
  const form = box.locator('[data-node-id="form"]');
  await form.getByRole('button', { name: 'Output folder: Choose a folder…' }).click();
  await expect(form.locator('[data-testid=path-text]')).toHaveAttribute('title', /bundle$/);
  await form.getByRole('button', { name: 'Publish token: Choose a secret…' }).click();
  await expect(form.getByTestId('view-chosen').nth(1)).toContainText('release-publish-token');
});

test('P2-11: the class diagram has a stereotype, a rule and left-aligned members', async ({ page }) => {
  const box = await open(page);
  const d = box.locator('[data-node-id="d1"]');
  await expect(d.locator('text.pd-view-scene__caption').first()).toHaveText('«model»');
  await expect(d.locator('line.pd-view-scene__rule')).toHaveCount(2);
  expect(await d.locator('text.pd-view-scene__detail').first().evaluate((e) => e.getAttribute('text-anchor'))).toBe('start');
});

test('P3-10: a scroll region is a tab stop only while it overflows', async ({ page }) => {
  const box = await open(page, 'dark', 1280);
  await expect(box.locator('[data-node-id="t1"] [role=region]')).not.toHaveAttribute('tabindex', /.*/);
  await page.setViewportSize({ width: 320, height: 900 });
  await expect(box.locator('[data-node-id="g1"] [role=region]')).toHaveAttribute('tabindex', '0');
});

for (const theme of THEMES) {
  test(`P2-12: the whole view, the review list included, fits 320 px (${theme})`, async ({ page }) => {
    const box = await open(page, theme, 320);
    const review = box.locator('[data-node-id="review"]');
    const over = await review.evaluate((root) => {
      const r = root.getBoundingClientRect();
      return Array.from(root.querySelectorAll<HTMLElement>('*')).filter((e) => e.getBoundingClientRect().right > r.right + 0.5 && !e.closest('.pd-view-review__filter')).map((e) => `${e.tagName.toLowerCase()}.${e.className} +${Math.round(e.getBoundingClientRect().right - r.right)} w${Math.round(e.getBoundingClientRect().width)} vs ${Math.round(r.width)}`);
    });
    expect(over).toEqual([]);
    expect(await review.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
    const list = review.locator('ul.pd-view-review');
    expect(await list.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
    // The filter is one row that scrolls in its own box (review 2, P3-5), and does not wrap.
    expect(await review.locator('.pd-view-review__filter').evaluate((e) => e.scrollWidth > e.clientWidth && getComputedStyle(e).overflowX === 'auto' && getComputedStyle(e).flexWrap === 'nowrap')).toBe(true);
    // The form's chosen text does not break in the middle of a word.
    expect(await box.locator('[data-node-id="form"] .pd-view-form__chosen').first().evaluate((e) => getComputedStyle(e).overflowWrap)).toBe('break-word');
  });
}

test('P2-4: stale is drawn as a warning, not as the danger of a failure', async ({ page }) => {
  const box = await open(page);
  await page.getByTestId('view-stale').click();
  await expect(box.getByTestId('view-error')).toHaveAttribute('data-kind', 'stale');
  const stale = await box.getByTestId('view-error').evaluate((e) => ({ border: getComputedStyle(e).borderTopColor, icon: getComputedStyle(e.querySelector('pd-glyph')!).color }));
  await page.getByTestId('view-bad').click();
  await expect(box.getByTestId('view-error')).toHaveAttribute('data-kind', 'failed');
  const read = () => box.getByTestId('view-error').evaluate((e) => ({ border: getComputedStyle(e).borderTopColor, icon: getComputedStyle(e.querySelector('pd-glyph')!).color }));
  await expect.poll(async () => (await read()).border).not.toBe(stale.border);
  const failed = await read();
  expect(failed.icon).toBe(failed.border);
  expect(stale.icon).toBe(stale.border);
});

// ---- Review 2 fixes (docs/review/x51-view/FIXES.md, review 2).

test('R2 P2-1: no edge of a graph or diagram runs through a box that is not one of its ends', async ({ page }) => {
  const box = await open(page);
  for (const id of ['g1', 'g2', 'g3', 'd1']) {
    const crossing = await box.locator(`[data-node-id="${id}"] svg.pd-view-scene__svg`).evaluate((svg) => {
      const rects = Array.from(svg.querySelectorAll<SVGRectElement>('g.pd-view-scene__node rect')).map((r) => ({ x: r.x.baseVal.value, y: r.y.baseVal.value, w: r.width.baseVal.value, h: r.height.baseVal.value }));
      const bad: string[] = [];
      for (const path of Array.from(svg.querySelectorAll<SVGPathElement>('path.pd-view-scene__edge'))) {
        const len = path.getTotalLength();
        // Skip the few units at either end: they are on the outline of the two boxes the edge joins.
        for (let d = 5; d < len - 5; d += 2) {
          const p = path.getPointAtLength(d);
          const hit = rects.findIndex((r) => p.x > r.x + 1 && p.x < r.x + r.w - 1 && p.y > r.y + 1 && p.y < r.y + r.h - 1);
          if (hit >= 0) {
            bad.push(`${path.getAttribute('d')} at ${Math.round(p.x)},${Math.round(p.y)} in box ${hit}`);
            break;
          }
        }
      }
      return bad;
    });
    expect(crossing, id).toEqual([]);
  }
});

for (const theme of THEMES) {
  test(`R2 P2-2: the matrix headings stay whole at 320 px, the cells scroll (${theme})`, async ({ page }) => {
    const box = await open(page, theme, 320);
    const heads = await box.locator('[data-node-id="matrix"] th').evaluateAll((ths) => ths.map((t) => ({ text: t.textContent?.trim() ?? '', width: t.getBoundingClientRect().width, height: t.getBoundingClientRect().height })));
    expect(heads.length).toBeGreaterThan(3);
    for (const h of heads) {
      expect(h.width, h.text).toBeGreaterThanOrEqual(96);
      // At most two lines of 12 to 13 px text plus padding: a heading is never cut into word fragments.
      expect(h.height, h.text).toBeLessThanOrEqual(60);
    }
  });

  test(`R2 P2-3 and R3 P2-1: a focus ring is never clipped by the region that scrolls or hides overflow around it, nor drawn over the text it rings (${theme})`, async ({ page }) => {
    const box = await open(page, theme);
    const found = await box.evaluate(async (root) => {
      const bad: string[] = [];
      const targets = Array.from(root.querySelectorAll<HTMLElement | SVGElement>('button, a[href], summary, input, select, textarea, [tabindex="0"], g[role=button]')).filter((e) => !e.closest('[inert]'));
      for (const el of targets) {
        if ((el as HTMLButtonElement).disabled) continue;
        (el as HTMLElement).focus({ preventScroll: false });
        // The ring may be transitioning in: measure it when every animation has finished.
        await Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined)));
        const cs = getComputedStyle(el);
        const grow = el instanceof SVGElement ? 0 : (cs.outlineStyle === 'none' ? 0 : parseFloat(cs.outlineWidth) + parseFloat(cs.outlineOffset));
        const r = el.getBoundingClientRect();
        const ring = { left: r.left - grow, top: r.top - grow, right: r.right + grow, bottom: r.bottom + grow };
        // Review 3, P2-1: an inset ring is a band along the edge of the element; it must not run over the element's own text or glyph.
        if (!(el instanceof SVGElement) && cs.outlineStyle !== 'none' && parseFloat(cs.outlineOffset) < 0) {
          const inset = -parseFloat(cs.outlineOffset);
          const range = document.createRange();
          range.selectNodeContents(el);
          const t = range.getBoundingClientRect();
          if (t.width > 0 && (t.left < r.left + inset - 0.01 || t.top < r.top + inset - 0.01 || t.right > r.right - inset + 0.01 || t.bottom > r.bottom - inset + 0.01)) {
            bad.push(`${el.tagName.toLowerCase()}.${el.getAttribute('class') ?? ''} "${(el.textContent ?? '').trim().slice(0, 24)}" ring band ${inset}px over its text ${JSON.stringify(t.toJSON())} in ${JSON.stringify(r.toJSON())}`);
          }
        }
        for (let p = el.parentElement; p && p !== root.parentElement; p = p.parentElement) {
          const o = getComputedStyle(p);
          if (/(hidden|auto|scroll|clip)/.test(o.overflowX + o.overflowY)) {
            const b = p.getBoundingClientRect();
            const inner = { left: b.left + parseFloat(o.borderLeftWidth), top: b.top + parseFloat(o.borderTopWidth), right: b.right - parseFloat(o.borderRightWidth), bottom: b.bottom - parseFloat(o.borderBottomWidth) };
            // Only the sides the element really sits against matter: a ring beyond the visible area of the clipping box is cut.
            if (ring.left < inner.left - 0.5 || ring.top < inner.top - 0.5 || ring.right > inner.right + 0.5 || ring.bottom > inner.bottom + 0.5) {
              bad.push(`${el.tagName.toLowerCase()}.${el.getAttribute('class') ?? ''} "${(el.textContent ?? '').trim().slice(0, 24)}" in ${p.tagName.toLowerCase()}.${p.getAttribute('class') ?? ''} ring ${JSON.stringify(ring)} inner ${JSON.stringify(inner)} focus-visible ${el.matches(':focus-visible')}`);
              break;
            }
          }
        }
      }
      return bad;
    });
    expect(found).toEqual([]);
  });
}

test('R2 P2-4: a hovered table row, tree row and drawing box change their fill (the pointer, no focus)', async ({ page }) => {
  const box = await open(page);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  const fill = (loc: Locator, prop: 'backgroundColor' | 'fill') => loc.evaluate((e, p) => getComputedStyle(e)[p as 'fill'], prop);
  const row = box.locator('[data-node-id="t1"] tbody tr').nth(1);
  const before = await fill(row, 'backgroundColor');
  await row.hover();
  expect(await fill(row, 'backgroundColor')).not.toBe(before);
  const tree = box.locator('[data-node-id="tree"] [role=treeitem]').nth(1);
  const treeBefore = await fill(tree, 'backgroundColor');
  await tree.hover();
  expect(await fill(tree, 'backgroundColor')).not.toBe(treeBefore);
  expect(await tree.evaluate((e) => e === document.activeElement)).toBe(false);
  const rect = box.locator('[data-node-id="g1"] g[role=button] rect').first();
  const rectBefore = await fill(rect, 'fill');
  await rect.hover();
  expect(await fill(rect, 'fill')).not.toBe(rectBefore);
});

test('R2 P3: tone glyph and word are apart in a table cell, the secret name sits next to its lock, the scatter unit is in the title only', async ({ page }) => {
  const box = await open(page);
  const gap = await box.locator('[data-node-id="t1"] td pd-view-tone').first().evaluate((e) => {
    const next = e.nextElementSibling as HTMLElement;
    return next.getBoundingClientRect().left - e.getBoundingClientRect().right;
  });
  expect(gap).toBeGreaterThanOrEqual(3);
  const form = box.locator('[data-node-id="form"]');
  await form.getByRole('button', { name: 'Publish token: Choose a secret…' }).click();
  const secret = form.locator('[data-testid=view-chosen]').nth(1);
  const lock = await secret.locator('pd-glyph').evaluate((e) => e.getBoundingClientRect().right);
  const name = await secret.locator('pd-path-text').evaluate((e) => e.getBoundingClientRect().left);
  expect(name - lock).toBeLessThanOrEqual(6);
  await expect(box.locator('[data-node-id="c5"]')).toContainText(/\(.+\)/);
});

test('R2 P3: at 320 px the review filter scrolls on one row, "Changes data" is the short badge, a wide table fades its far side and a wide drawing opens at its middle', async ({ page }) => {
  const box = await open(page, 'dark', 320);
  const filter = box.locator('.pd-view-review__filter');
  const rows = await filter.locator('button').evaluateAll((bs) => new Set(bs.map((b) => Math.round(b.getBoundingClientRect().top))).size);
  expect(rows).toBe(1);
  const writes = box.locator('[data-node-id="review"] pd-view-writes').first();
  expect(await writes.evaluate((e) => e.getBoundingClientRect().width)).toBeLessThan(80);
  expect(await writes.evaluate((e) => getComputedStyle(e.querySelector('.pd-view-writes__text')!, '::after').content)).toContain('Edits');
  const table = box.locator('[data-node-id="t1"] [role=region]');
  if (await table.evaluate((e) => e.scrollWidth > e.clientWidth + 1)) await expect(table).toHaveAttribute('data-more', 'end');
  const g2 = box.locator('[data-node-id="g2"] [role=region]');
  if (await g2.evaluate((e) => e.scrollWidth > e.clientWidth + 1)) expect(await g2.evaluate((e) => e.scrollLeft)).toBeGreaterThan(0);
});
