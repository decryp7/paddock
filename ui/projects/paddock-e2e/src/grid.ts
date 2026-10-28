import { expect, type Locator } from '@playwright/test';

/*
 * The "header stays on top" check (ER-21: the class of bug where scrolled rows or cells paint over a grid's sticky header).
 * It scrolls the grid's body in steps and then jumps more than two viewports, and at each position asks the browser which
 * element is on top at points across the header row (`document.elementFromPoint`): once in the first frame after the scroll
 * (before a virtual list or a sticky offset has caught up), and again once a drawn row reaches the header's bottom edge and
 * the row under the header is hovered (hover row actions, tints and focus rings are where stray stacking contexts come from). Every hit must be the header or inside it, and the header must paint an opaque
 * background, so nothing shows through it either. Works for `pd-table`, a `<table>` with a sticky `<thead>` and an app's
 * own div grid alike: the caller names the parts by selector, relative to the grid.
 */

/** Where a grid's parts are, as CSS selectors inside the grid's locator. */
export interface GridParts {
  /** The header row (or the `<thead>`). Default: `[role="row"]:first-child, thead`. */
  readonly header?: string;
  /**
   * The element that scrolls vertically, inside the grid. Default: the grid itself or its first descendant that can scroll, else
   * the nearest ancestor that can (a header that sticks to the page): that one is scrolled so the grid's rows pass under the header.
   */
  readonly scroller?: string;
  /** The body rows. Default: `[role="row"][data-index], tbody tr`. */
  readonly rows?: string;
  /** How many scroll steps to check (each step about a third of the visible body); a jump of more than two viewports follows. Default 6. */
  readonly steps?: number;
  /** Where the steps start in a body that scrolls inside the grid, in pixels (deep in a long virtual table). Default 0. */
  readonly from?: number;
}

/** One place where something other than the header was on top of it. */
export interface HeaderCover {
  /** The scroll position of the body. */
  readonly scrollTop: number;
  /** When it was seen: in the first frame after the scroll, or once the rows were drawn and one was hovered. */
  readonly phase: 'first-frame' | 'settled';
  /** The point checked, in viewport pixels. */
  readonly x: number;
  readonly y: number;
  /** A short description of the element found there instead (tag, classes, role). */
  readonly found: string;
}

/** What {@link headerCovers} found: the covers, the header's background, and how far the body really scrolled. */
export interface HeaderReport {
  readonly covers: readonly HeaderCover[];
  /** The header's computed background colour; it must be opaque. */
  readonly background: string;
  readonly opaque: boolean;
  /** The largest scroll position reached: 0 means the body never scrolled, so nothing was proven. */
  readonly scrolled: number;
}

/** Whether a computed CSS colour is opaque (`rgb()`, or `rgba()`/`color()` with alpha 1). */
export function isOpaqueColor(color: string): boolean {
  const c = color.trim();
  if (c === 'transparent' || c === '') return false;
  const slash = /\/\s*([\d.]+)(%?)\s*\)$/.exec(c);
  if (slash) return slash[2] === '%' ? Number(slash[1]) >= 100 : Number(slash[1]) >= 1;
  const rgba = /^rgba\(([^)]*)\)$/.exec(c);
  if (rgba) {
    const parts = rgba[1]!.split(',');
    return parts.length < 4 || Number(parts[3]) >= 1;
  }
  return true;
}

/** Scrolls, hovers and probes the header of `grid`; returns everything it found (see {@link expectHeaderOnTop}). */
export async function headerCovers(grid: Locator, parts: GridParts = {}): Promise<HeaderReport> {
  const page = grid.page();
  const headerSel = parts.header ?? '[role="row"]:first-child, thead';
  const rowSel = parts.rows ?? '[role="row"][data-index], tbody tr';
  const steps = parts.steps ?? 6;
  await grid.scrollIntoViewIfNeeded();
  const header = grid.locator(headerSel).first();
  await expect(header, 'the grid has a header').toBeVisible();

  const covers: HeaderCover[] = [];
  let scrolled = 0;
  // One page function does all of it (the page's CSP forbids passing code in as text): optionally scroll, wait one frame,
  // then find the box the header paints in and what is on top across it. The box is the union of the header cells (a
  // `<thead>`'s own box scrolls away with the table while its sticky `<th>` cells stay), cut to what its clipping ancestors
  // and the window show (a cell scrolled out sideways is hidden by its own scroller, not covered).
  const look = (scroll: { to: 'step' | 'jump'; step: number } | null) =>
    grid.evaluate(
      async (root, { headerSel, scrollerSel, scroll, steps, from }) => {
        let top = 0;
        if (scroll) {
          const canScroll = (e: Element) => e.scrollHeight > e.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(e).overflowY);
          const inside = scrollerSel ? root.querySelector(scrollerSel) : canScroll(root) ? root : [...root.querySelectorAll('*')].find(canScroll);
          if (inside) {
            inside.scrollTop = scroll.to === 'jump' ? inside.scrollTop + Math.round(inside.clientHeight * 2.5) + 13 : Math.round(from + (inside.clientHeight / 3) * scroll.step + 7);
            top = inside.scrollTop;
          } else {
            let outer = root.parentElement;
            while (outer && !canScroll(outer)) outer = outer.parentElement;
            const scroller = outer ?? document.scrollingElement;
            if (scroller) {
              // Put the grid's top above the scroller's top by a share of its height: the header sticks, the rows pass under it.
              const g = root.getBoundingClientRect();
              const s = scroller === document.scrollingElement ? 0 : scroller.getBoundingClientRect().top;
              const share = scroll.to === 'jump' ? Math.max(g.height * 0.9, scroller.clientHeight * 2.5) : (g.height * scroll.step) / (steps + 2);
              scroller.scrollTop += Math.round(g.top - s + Math.min(share, Math.max(0, g.height - scroller.clientHeight / 2)));
              top = scroller.scrollTop;
            }
          }
          // The first frame after the scroll: what the user sees before anything catches up.
          await new Promise<void>((done) => requestAnimationFrame(() => done()));
        }
        const h = root.querySelector(headerSel);
        if (!h) return { top, box: null, found: [] };
        const cells = [...h.querySelectorAll('th, [role="columnheader"]')].map((c) => c.getBoundingClientRect()).filter((r) => r.width > 0 && r.height > 0);
        const boxes = cells.length ? cells : [h.getBoundingClientRect()];
        let left = Math.min(...boxes.map((r) => r.left));
        let btop = Math.min(...boxes.map((r) => r.top));
        let right = Math.max(...boxes.map((r) => r.right));
        let bottom = Math.max(...boxes.map((r) => r.bottom));
        for (let a = h.parentElement; a; a = a.parentElement) {
          if (getComputedStyle(a).overflow === 'visible') continue;
          const r = a.getBoundingClientRect();
          [left, btop, right, bottom] = [Math.max(left, r.left), Math.max(btop, r.top), Math.min(right, r.right), Math.min(bottom, r.bottom)];
        }
        [left, btop, right, bottom] = [Math.max(left, 0), Math.max(btop, 0), Math.min(right, innerWidth), Math.min(bottom, innerHeight)];
        const box = { left, top: btop, width: Math.max(0, right - left), height: Math.max(0, bottom - btop) };
        const found: { x: number; y: number; found: string }[] = [];
        if (box.width && box.height) {
          for (const fy of [0.15, 0.5, 0.85]) {
            for (let fx = 0.02; fx <= 0.99; fx += 0.06) {
              const x = Math.round(box.left + box.width * fx);
              const y = Math.round(box.top + box.height * fy);
              const hit = document.elementFromPoint(x, y);
              if (!hit || h.contains(hit) || hit.contains(h)) continue;
              const role = hit.getAttribute('role');
              found.push({ x, y, found: `${hit.tagName.toLowerCase()}${hit.className && typeof hit.className === 'string' ? '.' + hit.className.trim().split(/\s+/).join('.') : ''}${role ? `[role=${role}]` : ''}` });
            }
          }
        }
        return { top, box, found };
      },
      { headerSel, scrollerSel: parts.scroller ?? null, scroll, steps, from: parts.from ?? 0 },
    );

  const positions = [...Array.from({ length: steps }, (_, i) => ({ to: 'step' as const, step: i + 1 })), { to: 'jump' as const, step: steps + 1 }];
  for (const position of positions) {
    const first = await look(position);
    scrolled = Math.max(scrolled, first.top);
    for (const f of first.found) covers.push({ scrollTop: first.top, phase: 'first-frame', ...f });
    if (!first.box?.width || !first.box.height) continue;
    // Wait for the condition, not for a number of frames: a drawn row reaches the header's bottom edge (the rows of the new
    // position are there and pass under the header).
    await grid.evaluate(
      (root, { rowSel, headerSel }) =>
        new Promise<void>((done, fail) => {
          const started = performance.now();
          const check = () => {
            const h = root.querySelector(headerSel);
            const edge = h ? Math.max(...[...h.querySelectorAll('th, [role="columnheader"]'), h].map((c) => c.getBoundingClientRect().bottom)) : 0;
            const reached = [...root.querySelectorAll(rowSel)].some((r) => {
              const b = r.getBoundingClientRect();
              return b.height > 0 && b.top <= edge + 1 && b.bottom >= edge - 1;
            });
            if (reached) return done();
            // A row that never comes is a failure of its own (nothing scrolled under the header); the test's own timeout names it.
            if (performance.now() - started > 10_000) return fail(new Error('no drawn row reached the header’s bottom edge'));
            requestAnimationFrame(check);
          };
          check();
        }),
      { rowSel, headerSel },
    );
    // Hover the row whose box reaches past the header: the row that scrolled under it.
    const hbox = first.box;
    const rows = grid.locator(rowSel);
    const count = Math.min(await rows.count(), 200);
    for (let i = 0; i < count; i++) {
      const box = await rows.nth(i).boundingBox();
      if (box && box.y + box.height > hbox.top + hbox.height + 2 && box.height > 0) {
        await page.mouse.move(box.x + box.width / 2, Math.max(box.y + 2, hbox.top + hbox.height + 2));
        break;
      }
    }
    const settled = await look(null);
    for (const f of settled.found) covers.push({ scrollTop: first.top, phase: 'settled', ...f });
  }
  await page.mouse.move(0, 0);
  const background = await header.evaluate((h) => {
    // A `<thead>` paints through its cells: the first header cell's background is the one that shows.
    const cell = h.tagName === 'THEAD' ? (h.querySelector('th') ?? h) : h;
    return getComputedStyle(cell).backgroundColor;
  });
  return { covers, background, opaque: isOpaqueColor(background), scrolled };
}

/**
 * Fails when rows or cells paint over the grid's header while it scrolls and a row is hovered, when the header's
 * background lets them show through, or when the body did not scroll at all (the check would prove nothing).
 *
 * ```ts
 * await expectHeaderOnTop(page.getByRole('grid', { name: 'Plugins' }));
 * await expectHeaderOnTop(page.locator('.processes'), { header: 'thead', scroller: '.scroll' });
 * ```
 */
export async function expectHeaderOnTop(grid: Locator, parts: GridParts = {}): Promise<void> {
  const report = await headerCovers(grid, parts);
  expect(report.scrolled, 'the grid body scrolled (give it more rows than fit)').toBeGreaterThan(0);
  expect(report.opaque, `the header background is opaque (${report.background})`).toBe(true);
  expect(report.covers, 'nothing paints over the header while the rows scroll and one is hovered').toEqual([]);
}
