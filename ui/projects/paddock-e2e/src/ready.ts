import { expect, type Locator, type Page } from '@playwright/test';
import { settledLayout } from './settled-layout.ts';

/**
 * "Ready" per kind of thing a spec reads or presses. `toBeVisible()` says an element is drawn; it does not say its content, its position or
 * its wiring is final (the sweep of 2026-11-17, docs/lessons.md: a palette still showing the unfiltered list, an overlay not yet positioned, a
 * section whose update pass had not run, an answer still streaming above a card). Wait with the helper of the kind, then read or press. The
 * lint rule `paddock/e2e-ready-reads` (warn) flags a read right after a bare visibility wait.
 */

/** The larger of the offset of `box` from `anchor`'s left edge and the difference of their widths, in px (pure; `null` when either is not drawn). */
export function overlayOffset(box: { x: number; width: number } | null, anchor: { x: number; width: number } | null): number | null {
  if (!box || !anchor) return null;
  return Math.max(Math.abs(box.x - anchor.x), Math.abs(box.width - anchor.width));
}

/**
 * Waits until a list is the filtered one: its first option carries `marker` (default `mark`, the highlight of the matched text, which the
 * unfiltered list has none of). A list read on "an option is visible" can be the one before the query was applied.
 */
export async function expectListFiltered(options: Locator, marker = 'mark'): Promise<void> {
  await expect(options.first().locator(marker), 'the list is the filtered one (its first option marks the match)').toBeVisible();
}

/**
 * Waits until an overlay (a select list, a menu) lines up with its anchor within `tolerance` px in its left edge and its width. The panel is
 * visible a frame or two before the overlay positions it (at x 0, 131 px wide, then at the field's x and width). Fails with the last offset.
 */
export async function expectOverlayPositioned(panel: Locator, anchor: Locator, tolerance = 1): Promise<void> {
  await expect
    .poll(async () => overlayOffset(await panel.boundingBox(), await anchor.boundingBox()) ?? Number.POSITIVE_INFINITY, {
      message: `the overlay lines up with its anchor: its left edge or width differs by more than ${tolerance} px (or one is not drawn)`,
    })
    .toBeLessThanOrEqual(tolerance);
}

/**
 * Waits until a control is wired: the attribute only the update pass sets is there (default `aria-expanded` of a select or a menu trigger,
 * "false"). A control in the DOM from the creation pass answers a click or a key with "onContainerClick of undefined".
 */
export async function expectWired(control: Locator, attribute = 'aria-expanded', value: string | RegExp = 'false'): Promise<void> {
  await expect(control, `the control is wired (${attribute} is set by the update pass)`).toHaveAttribute(attribute, value);
}

/** Waits until nothing in `busy` is shown any more (a Stop button, a "thinking" line): an answer that still streams moves what is above it. */
export async function expectIdle(...busy: Locator[]): Promise<void> {
  for (const b of busy) await expect(b, 'the stream has finished').toHaveCount(0);
}

/** Waits until the dock has planned its arrangement for the window's current size (`data-dock-planned`, Debrief's DockService.planned). */
export async function expectDockPlanned(page: Page): Promise<void> {
  await expect(page.locator('[data-dock-planned]'), 'the dock has planned for the current window size').toBeAttached();
}

/** Waits until an element's box (left, top, width, height) is the same in two reads in a row: it is drawn and not moving (`settledLayout`). */
export async function expectNotMoving(target: Locator, what = 'the element'): Promise<void> {
  await settledLayout(
    async () => {
      const b = await target.boundingBox();
      return { count: b ? 1 : 0, widths: b ? [b.x, b.y, b.width, b.height].map(Math.round) : [], fits: true };
    },
    { what },
  );
}
