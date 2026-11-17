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
  readonly left: number;
  readonly width: number;
  readonly heights: readonly number[];
}

// Not covered yet, an open finding (docs/lessons.md): 920 to 934 px wide. There the layout plan keeps the Explorer when the first frame is drawn and
// closes it by the dock's first plan, and dockview lays the dock out at the new container size three frames later (the dock's left edge moves 320 to 48
// and its width 580 to 852 in drawn frames, measured with requestAnimationFrame). The constant of NarrowWindow.start (920) stops the jump below that.
for (const size of [
  { width: 1440, height: 900 },
  { width: 1100, height: 900 },
  { width: 1000, height: 700 },
  { width: 900, height: 900 },
  { width: 700, height: 900 },
]) {
  test(`the default layout does not move after the app is ready at ${size.width}x${size.height}`, async ({ page }) => {
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
        const box = dock?.getBoundingClientRect();
        const bar = !!document.querySelector('debrief-filter-bar');
        const planned = !!document.querySelector('debrief-dock-host[data-dock-planned]');
        const heights = [...document.querySelectorAll('.dv-groupview')].map((g) => Math.round(g.getBoundingClientRect().height));
        const reading = { bar, planned, top: box ? Math.round(box.top) : -1, left: box ? Math.round(box.left) : -1, width: box ? Math.round(box.width) : -1, heights };
        const key = JSON.stringify(reading);
        if (key !== last) log.push(reading);
        last = key;
      };
      // Read after the frame's other observers have run (the dock's own included), not between them.
      const later = () => setTimeout(record, 0);
      const resized = new ResizeObserver(later);
      const watch = (): void => {
        for (const el of document.querySelectorAll('.dv-dockview, .dv-groupview, debrief-filter-bar, debrief-dock-host')) resized.observe(el);
      };
      // The signal itself changes no size, so its attribute is observed too.
      new MutationObserver(later).observe(document, { subtree: true, attributes: true, attributeFilter: ['data-dock-planned'] });
      new MutationObserver(watch).observe(document, { childList: true, subtree: true });
      watch();
    });
    await openApp(page);
    await expect(page.locator('debrief-filter-bar')).toBeVisible();
    await expectDockPlanned(page);
    const readLog = () => page.evaluate(() => (window as unknown as { __dockLog: Reading[] }).__dockLog);
    // The recorder reads a task after the attribute changes: wait until it has recorded the planned dock before judging the log.
    await expect.poll(async () => (await readLog()).some((r) => r.planned && r.bar), { message: 'the recorder saw the planned dock' }).toBe(true);
    const log = await readLog();
    const shown = (r: Reading) => JSON.stringify(r);
    const at = (r: Reading) => `${r.top} ${r.left} ${r.width}`;
    // The dock's own first plan may add the auto-hide bar beside it (28 px) before it is planned, so its width is judged from the planned reading on.
    const outside = (r: Reading) => `${r.top} ${r.left}`;
    // From the first reading with the filter bar and the groups, the dock's top and left edge never change, at any width: the Explorer's
    // state (closed in a narrow window, by the layout plan's own decision) is made before the first draw, so nothing above or beside the dock changes.
    const withBar = log.filter((r) => r.bar && r.heights.length >= GROUPS);
    expect(withBar.length, `no reading had the filter bar and ${GROUPS} groups: ${log.map(shown).join(' ')}`).toBeGreaterThan(0);
    expect(new Set(withBar.map(outside)).size, `the dock's top or left edge moved: ${withBar.map(shown).join(' ')}`).toBe(1);
    // From the first reading with the dock planned (and the bar drawn), neither its box nor a group's height changes.
    const first = log.findIndex((r) => r.planned && r.bar && r.heights.length >= GROUPS);
    expect(first, `the dock was never seen planned with the bar and ${GROUPS} groups: ${log.map(shown).join(' ')}`).toBeGreaterThanOrEqual(0);
    const planned = log.slice(first);
    expect(new Set(planned.map((r) => `${at(r)} ${r.heights.join('/')}`)).size, `the layout moved after the dock was planned: ${planned.map(shown).join(' ')}`).toBe(1);
    // The grid keeps its share and the timeline row takes the rest.
    expect(Math.min(...planned[0]!.heights)).toBeGreaterThan(200);
  });
}
