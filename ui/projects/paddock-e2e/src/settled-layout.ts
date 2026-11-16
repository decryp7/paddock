import { expect, type Page } from '@playwright/test';

/** What one look at a page's layout found: how many boxes were drawn, how wide each is, and whether every one fits. */
export interface LayoutReading {
  /** The boxes drawn (rows, tables, headers): the number the spec expects to be above 0. */
  readonly count: number;
  /** The width of each box, in order (rounded). */
  readonly widths: readonly number[];
  /** Whether every box fits (its text fitted its width, its columns fitted its table). A reading of no boxes never fits. */
  readonly fits: boolean;
}

/** Options of {@link settledLayout}. */
export interface SettledLayoutOptions {
  /** The fewest boxes a settled page has (default 1): a page with none drawn yet is not settled, however much agrees. */
  readonly minCount?: number;
  /** What the spec waits for, in the failure message (for example `the firewall page`). */
  readonly what?: string;
  /** How long to wait for two agreeing readings (default Playwright's `expect` timeout). */
  readonly timeout?: number;
}

/**
 * Whether `now` settles a layout after `previous`: both have at least `minCount` boxes (never vacuous: `every()` over no boxes is
 * true, and a spec that reads layout before any row is drawn measures nothing), the same count and widths, and every box fits.
 */
export function layoutSettled(previous: LayoutReading | null, now: LayoutReading, minCount = 1): boolean {
  if (previous === null || now.count < Math.max(1, minCount) || previous.count !== now.count || !now.fits || !previous.fits) return false;
  return previous.widths.length === now.widths.length && previous.widths.every((w, i) => w === now.widths[i]);
}

/**
 * Waits until the layout stops changing and returns the settled reading. `read` looks at the page once (in one `evaluate`, so the
 * boxes and their widths are of the same moment); the layout is settled when two reads in a row agree (the same boxes and widths, all fit,
 * and at least `minCount` boxes). A page's ResizeObservers and virtual rows settle over frames the spec cannot name, and a slower timer (the
 * pinned fake clock's `setTimeout(0)` takes about twice as long) shows a spec that measured too early: 31 of 240 Firewall visits in Dyno's
 * layout spec, 13%. Wait on this instead of reading once after `open()`.
 */
export async function settledLayout(page: Page, read: () => Promise<LayoutReading>, options: SettledLayoutOptions = {}): Promise<LayoutReading> {
  void page;
  let previous: LayoutReading | null = null;
  let settled: LayoutReading | null = null;
  await expect
    .poll(
      async () => {
        const now = await read();
        settled = layoutSettled(previous, now, options.minCount) ? now : null;
        previous = now;
        return settled !== null;
      },
      { message: `${options.what ?? 'the page'}: the layout settles (two reads agree on the boxes and their widths, all fit, at least ${Math.max(1, options.minCount ?? 1)} drawn)`, ...(options.timeout ? { timeout: options.timeout } : {}) },
    )
    .toBe(true);
  return settled as unknown as LayoutReading;
}
