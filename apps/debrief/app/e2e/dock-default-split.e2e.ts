import { expect, test } from './test';
import { openApp } from './app';
import { expectDockPlanned } from '../../../../ui/projects/paddock-e2e/src/ready.ts';

// The default split is applied once the dock has its size, so the layout is the same the moment the app is ready and
// a moment later (it used to settle at 300/489 or 282/507 px depending on when the container was measured).
// "Ready" is the dock's own signal, `data-dock-planned` on debrief-dock-host (DockService.planned: the first fit has run and no resize waits
// for its re-plan), not a wait or a settled read. The filter bar is above the dock and is not deferred: its height (one row, or two when it wraps
// in a narrow window) is in the layout from the first frame it exists, so the dock's top never moves after it. A fixed placeholder of one
// row's height moved the dock by 36 to 72 px at 900 to 1100 px wide.

/** The default layout has at least the grid and the timeline (the inspector is a tab beside them in a narrow window): every reading below must see both. */
const GROUPS = 2;

interface Reading {
  readonly bar: boolean;
  readonly planned: boolean;
  readonly top: number;
  readonly heights: readonly number[];
}

for (const size of [
  { width: 1440, height: 900 },
  { width: 1100, height: 900 },
  { width: 900, height: 900 },
  { width: 700, height: 900 },
]) {
  test(`the default layout does not move after the app is ready at ${size.width} px wide`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.addInitScript(() => {
      localStorage.clear();
      // One record each time the dock, a group or the filter bar is resized, from the first moment on. A resize observer reports the layout of a
      // frame that is about to be drawn; a mutation observer would also report states that are never drawn (a bar that is wide for a task, then compact).
      const log: Reading[] = [];
      (window as unknown as { __dockLog: Reading[] }).__dockLog = log;
      let last = '';
      const record = (): void => {
        const dock = document.querySelector('.dv-dockview');
        const bar = !!document.querySelector('debrief-filter-bar');
        const planned = !!document.querySelector('debrief-dock-host[data-dock-planned]');
        const top = dock ? Math.round(dock.getBoundingClientRect().top) : -1;
        const heights = [...document.querySelectorAll('.dv-groupview')].map((g) => Math.round(g.getBoundingClientRect().height));
        const key = JSON.stringify([bar, planned, top, heights]);
        if (key !== last) log.push({ bar, planned, top, heights });
        last = key;
      };
      // Read after the frame's other observers have run (the dock's own included), not between them.
      const resized = new ResizeObserver(() => setTimeout(record, 0));
      const watch = (): void => {
        for (const el of document.querySelectorAll('.dv-dockview, .dv-groupview, debrief-filter-bar')) resized.observe(el);
      };
      new MutationObserver(watch).observe(document, { childList: true, subtree: true });
      watch();
    });
    await openApp(page);
    await expect(page.locator('debrief-filter-bar')).toBeVisible();
    await expectDockPlanned(page);
    const log = await page.evaluate(() => (window as unknown as { __dockLog: Reading[] }).__dockLog);
    const shown = (r: Reading) => `${JSON.stringify(r)}`;
    // From the first reading with the filter bar and all groups, the dock's top never changes.
    const withBar = log.filter((r) => r.bar && r.heights.length >= GROUPS);
    expect(withBar.length, `no reading had the filter bar and ${GROUPS} groups: ${log.map(shown).join(' ')}`).toBeGreaterThan(0);
    // From the first reading with the filter bar and the groups, the dock's top never changes, at any width: the Explorer's state (closed in a
    // narrow window) is decided before the first draw, so nothing above or beside the dock changes once it is drawn.
    expect(new Set(withBar.map((r) => r.top)).size, `the dock's top moved: ${withBar.map(shown).join(' ')}`).toBe(1);
    // From the first reading with the dock planned (and the bar drawn), neither the top nor a group's height changes.
    const planned = log.slice(log.findIndex((r) => r.planned && r.bar && r.heights.length >= GROUPS));
    expect(planned.length, `the dock was never planned with the bar and ${GROUPS} groups: ${log.map(shown).join(' ')}`).toBeGreaterThan(0);
    expect(new Set(planned.map((r) => `${r.top} ${r.heights.join('/')}`)).size, `the layout moved after the dock was planned: ${planned.map(shown).join(' ')}`).toBe(1);
    // The grid keeps its share and the timeline row takes the rest.
    expect(Math.min(...planned[0]!.heights)).toBeGreaterThan(200);
  });
}
