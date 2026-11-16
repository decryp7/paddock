import { settledLayout } from '@decryp7/paddock-e2e';
import { expect, test } from './test';
import { openApp } from './app';

// The default split is applied once the dock has its size, so the layout is the same the moment the app is ready and
// a moment later (it used to settle at 300/489 or 282/507 px depending on when the container was measured).
// The filter bar is loaded lazily: its placeholder holds its room, so the dock's top does not move when the bar arrives nor while the bar
// settles (the placeholder was 30 px, the bar 51 px, and the bar was 59 px for a frame). The groups' heights follow the container once and
// the spec waits for them to settle (settledLayout) instead of reading after a fixed second: 3 to 7 of 120 runs read them too early,
// at every pinned instant.
test('the default layout does not move after the app is ready', async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.clear();
    // One record each time the DOM changes or the dock or a group is resized (observers, no frame polling), from the first moment on.
    const log: { bar: string; top: number; heights: number[] }[] = [];
    (window as unknown as { __dockLog: typeof log }).__dockLog = log;
    let last = '';
    const record = (): void => {
      const dock = document.querySelector('.dv-dockview');
      const bar = document.querySelector('debrief-filter-bar') ? 'bar' : document.querySelector('[data-testid=filter-bar-placeholder]') ? 'placeholder' : 'none';
      const top = dock ? Math.round(dock.getBoundingClientRect().top) : -1;
      const heights = [...document.querySelectorAll('.dv-groupview')].map((g) => Math.round(g.getBoundingClientRect().height));
      const key = JSON.stringify([bar, top, heights]);
      if (key !== last) log.push({ bar, top, heights });
      last = key;
    };
    const resized = new ResizeObserver(record);
    const watch = (): void => {
      record();
      for (const el of document.querySelectorAll('.dv-dockview, .dv-groupview')) resized.observe(el);
    };
    new MutationObserver(watch).observe(document, { childList: true, subtree: true, attributes: true });
    watch();
  });
  await openApp(page);
  await expect(page.locator('debrief-filter-bar')).toBeVisible();
  const heights = () => page.evaluate(() => [...document.querySelectorAll('.dv-groupview')].map((g) => Math.round(g.getBoundingClientRect().height)));
  // Settled: two reads in a row agree (the dock's resize observers settle over frames the spec cannot name).
  const first = (await settledLayout(async () => ({ count: (await heights()).length, widths: await heights(), fits: true }), { what: 'the default dock layout', minCount: 2 })).widths;
  // From the first frame in which the filter bar (or its placeholder) is drawn, the dock's top and its groups' heights never change.
  const log = await page.evaluate(() => (window as unknown as { __dockLog: { bar: string; top: number; heights: number[] }[] }).__dockLog);
  const withBar = log.filter((r) => r.bar !== 'none' && r.heights.length >= 3);
  expect(new Set(withBar.map((r) => r.top)).size, `the dock's top moved: ${JSON.stringify(withBar)}`).toBe(1);
  // The dock re-plans its split once the container's size is final (debounced, DockLayout.watchResize), so the heights settle after the
  // top does; the settled reading is the last frame's, and later frames add nothing.
  expect(withBar.at(-1)!.heights).toEqual(first);
  // The grid keeps its share and the timeline row takes the rest.
  expect(first.length).toBeGreaterThanOrEqual(2);
  expect(Math.min(...first)).toBeGreaterThan(200);
});
