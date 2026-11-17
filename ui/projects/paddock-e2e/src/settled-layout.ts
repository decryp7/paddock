import { expect } from '@playwright/test';

/** What one look at a page's layout found: how many boxes were drawn, how wide each is, and whether every one fits. */
export interface LayoutReading {
  /** The boxes drawn (rows, tables, headers): the number the spec expects to be above 0. */
  readonly count: number;
  /** The width of each box, in order (rounded). */
  readonly widths: readonly number[];
  /** Whether every box fits (its text fitted its width, its columns fitted its table). Count 0 with `minCount` 0 (a page that draws no boxes by design) can fit; with the default `minCount` it never settles. */
  readonly fits: boolean;
}

/** Options of {@link settledLayout}. */
export interface SettledLayoutOptions {
  /**
   * The fewest boxes a settled page has (default 1): a page with none drawn yet is not settled, however much agrees. Say `0` only for a
   * page that draws none by design (a text page), so an empty reading there is the settled one.
   */
  readonly minCount?: number;
  /** What the spec waits for, in the failure message (for example `the firewall page`). */
  readonly what?: string;
  /** The widths the settled reading is expected to have, when the spec knows them (the row heights of a design): the failure message shows them beside the last reading. */
  readonly expectedWidths?: readonly number[];
  /** How long to wait for two agreeing readings (default Playwright's `expect` timeout). */
  readonly timeout?: number;
}

/**
 * Whether `now` settles a layout after `previous`: both have at least `minCount` boxes (default 1, never vacuous: `every()` over no boxes
 * is true, and a spec that reads layout before any row is drawn measures nothing), the same count and widths, and every box fits.
 */
export function layoutSettled(previous: LayoutReading | null, now: LayoutReading, minCount = 1): boolean {
  if (previous === null || now.count < minCount || previous.count !== now.count || !now.fits || !previous.fits) return false;
  return previous.widths.length === now.widths.length && previous.widths.every((w, i) => w === now.widths[i]);
}

/**
 * Waits until the layout stops changing and returns the settled reading. `read` looks at the page once (in one `evaluate`, so the
 * boxes and their widths are of the same moment); the layout is settled when two reads in a row agree (the same boxes and widths, all fit,
 * and at least `minCount` boxes). A page's ResizeObservers and virtual rows settle over frames the spec cannot name, and a slower timer (the
 * pinned fake clock's `setTimeout(0)` takes about twice as long) shows a spec that measured too early: 31 of 240 Firewall visits in Dyno's
 * layout spec, 13%. Wait on this instead of reading once after `open()`.
 */
export async function settledLayout(read: () => Promise<LayoutReading>, options: SettledLayoutOptions = {}): Promise<LayoutReading> {
  const minCount = options.minCount ?? 1;
  const what = options.what ?? 'the page';
  let previous: LayoutReading | null = null;
  let last: LayoutReading | null = null;
  let settled: LayoutReading | null = null;
  try {
    await expect
      .poll(
        async () => {
          const now = await read();
          // Against the read just before this one (`last`): two reads in a row. `previous` only keeps the read before that for the failure message.
          settled = layoutSettled(last, now, minCount) ? now : null;
          previous = last;
          last = now;
          return settled !== null;
        },
        { message: `${what}: the layout settles`, ...(options.timeout ? { timeout: options.timeout } : {}) },
      )
      .toBe(true);
  } catch (e) {
    const show = (r: LayoutReading | null) => (r ? `${r.count} boxes, fit ${r.fits}, widths [${r.widths.join(', ')}]` : 'none');
    throw new Error(
      `${what}: the layout did not settle (two reads must agree on the boxes and their widths, all fit, at least ${minCount} drawn).\n  last reading:     ${show(last)}\n  reading before:   ${show(previous)}` +
        (options.expectedWidths ? `\n  expected widths:  [${options.expectedWidths.join(', ')}]` : ''),
      { cause: e },
    );
  }
  return settled as unknown as LayoutReading;
}
