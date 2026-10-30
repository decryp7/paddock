import { expect, test } from '@decryp7/paddock-e2e';

/**
 * Rule 18 at the laptop widths a window really has (900, 1024 and 1280): on every page of every mock scenario nothing is
 * wider than its page and no text is cut off without an ellipsis.
 */
const WIDTHS = [900, 1024, 1280];
/** [scenario, route, label, what shows once the page has its data]. */
const PAGES: [string, string, string, string][] = [
  ['demo', '/', 'dashboard', '.strip'],
  ['demo', '/#/watches', 'watches', 'tbody tr'],
  ['demo', '/#/watches/w-1', 'watch detail', 'main h1'],
  ['demo', '/#/watches/w-1/edit', 'edit watch', 'main form, main input'],
  ['demo', '/#/watches/new', 'add watch', 'main form, main input'],
  ['empty', '/', 'empty dashboard', '.empty'],
  ['empty', '/#/watches', 'empty watches', 'main h1'],
  ['broken', '/', 'broken dashboard', 'main [role=alert]'],
  ['broken', '/#/watches', 'broken watches', 'main h1'],
  ['products', '/#/watches/w-9', 'products detail', 'main h1'],
];

/** The elements that stick out past the viewport or whose text is cut without an ellipsis. */
const FINDINGS = `(() => {
  const vw = document.documentElement.clientWidth;
  const out = [];
  const name = (e) => e.tagName.toLowerCase() + (e.className && typeof e.className === 'string' ? '.' + e.className.trim().split(/\\s+/).join('.') : '');
  // The right edge an element must stay within: the nearest ancestor that clips (or the window); inside a scroller it may be wider.
  const limit = (e) => {
    for (let p = e.parentElement; p; p = p.parentElement) {
      const o = getComputedStyle(p).overflowX;
      if (o === 'auto' || o === 'scroll') return Infinity;
      if (o === 'hidden' || o === 'clip') return p.getBoundingClientRect().left + p.clientWidth;
    }
    return vw;
  };
  for (const e of document.querySelectorAll('body *')) {
    const cs = getComputedStyle(e);
    if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    const r = e.getBoundingClientRect();
    if (r.width === 0 || r.height === 0 || e.clientWidth <= 1) continue;
    if (r.right > limit(e) + 2) out.push('wider than the page: ' + name(e) + ' right=' + Math.round(r.right));
    const clips = cs.overflowX === 'hidden' || cs.overflowX === 'clip';
    if (clips && e.scrollWidth > e.clientWidth + 1 && cs.textOverflow !== 'ellipsis' && e.textContent && e.textContent.trim()) {
      out.push('text clipped without an ellipsis: ' + name(e) + ' ' + e.scrollWidth + '>' + e.clientWidth);
    }
  }
  return out;
})()`;

for (const width of WIDTHS) {
  test.describe(`at ${width} px`, () => {
    for (const [scenario, route, label, ready] of PAGES) {
      test(`${label} (${scenario}) has nothing wider than the page and no silently clipped text`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        const sep = route.startsWith('/#') ? route.replace('/#', `/?mock=${scenario}#`) : `/?mock=${scenario}`;
        await page.goto(sep);
        await expect(page.locator(ready).first()).toBeVisible();
        expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth), 'horizontal scroll').toBeLessThanOrEqual(0);
        expect(await page.evaluate(FINDINGS)).toEqual([]);
      });
    }
  });
}

/** Dialog windows at the size their contract declares (rule 18 applies to them too): nothing wider than the window, no silently clipped text. */
const WINDOWS: [string, string, { width: number; height: number }, unknown][] = [
  ['settings', 'pd.settings', { width: 880, height: 640 }, {}],
  ['updates', 'pd.updates', { width: 560, height: 480 }, {}],
  ['about', 'pd.about', { width: 680, height: 560 }, {}],
  ['whats new', 'pd.whats-new', { width: 680, height: 560 }, {}],
  ['confirm', 'pd.confirm', { width: 420, height: 160 }, { heading: 'Delete "Lumen CLI"?', message: 'Spotter stops watching it and forgets its history. Downloaded files stay where they are.', confirm: { label: 'Delete watch', destructive: true } }],
];
for (const [label, id, size, input] of WINDOWS) {
  test(`the ${label} window has nothing wider than itself and no silently clipped text`, async ({ page, app }) => {
    await page.setViewportSize(size);
    await app.dialogs.openWindow(id, input);
    await app.dialogs.loaded();
    expect(await page.evaluate(FINDINGS)).toEqual([]);
  });
}

test('the Spotter page of the Settings window has no clipped controls', async ({ page, app }) => {
  await page.setViewportSize({ width: 880, height: 640 });
  await app.settings.open({ page: 'sp.general' });
  await expect(page.getByRole('radiogroup', { name: 'Digest' })).toBeVisible();
  expect(await page.evaluate(FINDINGS)).toEqual([]);
});
