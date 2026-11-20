import { expect, test, type Locator, type Page } from '@playwright/test';
import { settledLayout } from './settled-layout.ts';

/**
 * "Ready" per kind of thing a spec reads or presses. `toBeVisible()` says an element is drawn; it does not say its content, its position or
 * its wiring is final (the sweep of 2026-11-17, docs/lessons.md: a palette still showing the unfiltered list, an overlay not yet positioned, a
 * section whose update pass had not run, an answer still streaming above a card). Wait with the helper of the kind, then read or press. The
 * lint rule `paddock/e2e-ready-reads` (an error) flags a read right after a bare visibility wait.
 */

/** The larger of the offset of `box` from `anchor`'s left edge and the difference of their widths, in px (pure; `null` when either is not drawn). */
export function overlayOffset(box: { x: number; width: number } | null, anchor: { x: number; width: number } | null): number | null {
  if (!box || !anchor) return null;
  return Math.max(Math.abs(box.x - anchor.x), Math.abs(box.width - anchor.width));
}

/**
 * Waits until a list is the filtered one for `query`: its first option highlights the match (a `mark` whose text is the query, in any case).
 * The unfiltered list (recent and common commands) has no highlight, and a highlight of another query is not this one.
 */
export async function expectListFiltered(options: Locator, query: string): Promise<void> {
  await expect(options.first().locator('mark').first(), `the list is filtered for "${query}" (its first option highlights it)`).toContainText(query, { ignoreCase: true });
}

/**
 * Waits until an overlay (a select list, a menu) is aligned with its anchor: the same left edge and width within `tolerance` px (a select's list
 * is as wide as its field and starts at its edge; this is not for a menu that opens below and to the side). The panel is visible a frame or two
 * before the overlay positions it (at x 0, 131 px wide, then at the field's x and width). Fails with the last offset.
 */
export async function expectOverlayAlignedWith(panel: Locator, anchor: Locator, tolerance = 1): Promise<void> {
  await expect
    .poll(async () => overlayOffset(await panel.boundingBox(), await anchor.boundingBox()) ?? Number.POSITIVE_INFINITY, {
      message: `the overlay lines up with its anchor: its left edge or width differs by more than ${tolerance} px (or one is not drawn)`,
    })
    .toBeLessThanOrEqual(tolerance);
}

type Box = { x: number; y: number; width: number; height: number };

/**
 * How far a menu is from sitting against its trigger, in px (pure; `null` when either is not drawn): the smallest distance between an edge of
 * `panel` and the facing edge of `anchor`, over the sides where the two overlap along the edge (below or above a menu-bar item, beside a
 * submenu item). A panel that covers its anchor is not positioned yet (CDK draws an overlay at the top-left until its first `apply()`, which
 * comes with Angular's next render): `Infinity`, as is a panel that touches no side of it.
 */
export function menuGap(panel: Box | null, anchor: Box | null): number | null {
  if (!panel || !anchor) return null;
  const overlapX = Math.min(panel.x + panel.width, anchor.x + anchor.width) - Math.max(panel.x, anchor.x);
  const overlapY = Math.min(panel.y + panel.height, anchor.y + anchor.height) - Math.max(panel.y, anchor.y);
  if (overlapX > 0 && overlapY > 0) return Number.POSITIVE_INFINITY;
  const gaps: number[] = [];
  if (overlapX > 0) gaps.push(Math.abs(panel.y - (anchor.y + anchor.height)), Math.abs(panel.y + panel.height - anchor.y));
  if (overlapY > 0) gaps.push(Math.abs(panel.x - (anchor.x + anchor.width)), Math.abs(panel.x + panel.width - anchor.x));
  return gaps.length ? Math.min(...gaps) : Number.POSITIVE_INFINITY;
}

/**
 * Waits until a menu sits against its trigger (`menuGap` within `tolerance` px): below or above a menu-bar item, beside a submenu item. The menu is
 * visible, and stays still, at the top-left of the window until the overlay is positioned, which waits for the app's next render (Debrief's File
 * menu sat there for about 500 ms during the first load in WebKit, debrief.yml run 8297); a click aimed there lands on another item once the menu
 * moves. Fails with the last gap.
 */
export async function expectMenuAnchoredTo(panel: Locator, anchor: Locator, tolerance = 2): Promise<void> {
  await expect
    .poll(async () => menuGap(await panel.boundingBox(), await anchor.boundingBox()) ?? Number.POSITIVE_INFINITY, {
      message: `the menu sits against its trigger: no edge of it within ${tolerance} px of the trigger's facing edge (or it covers the trigger, or one is not drawn)`,
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

/** What a stream has shown so far: its text length and element count. A change between two reads means something new arrived. */
export interface StreamProgress {
  readonly chars: number;
  readonly elements: number;
}

/** Did anything arrive between two reads of a stream (pure)? */
export function streamGrew(before: StreamProgress, after: StreamProgress): boolean {
  return before.chars !== after.chars || before.elements !== after.elements;
}

async function readStreamProgress(stream: Locator): Promise<StreamProgress> {
  return stream.evaluate((el) => ({ chars: el.textContent?.length ?? 0, elements: el.getElementsByTagName('*').length }));
}

/**
 * Waits until a stream has finished: its container's `aria-busy` is "false" (the standard ARIA pattern for a live region being updated; Debrief's
 * chat log sets it while the turn runs). Precondition: the stream has started, so a turn is already drawn (the caller waited for it, as
 * Debrief's `ask` does): before that `aria-busy` is "false" too, and this would pass at once.
 *
 * The bound is on a stall, not on the whole stream: each wait lasts the expect timeout (or `timeout`), and the next one starts only when something
 * new arrived during it (text or elements). A stream's length is the app's script times the runner's speed: Debrief's 20-piece answer, 150 ms
 * apart, took 3.4 s on an idle Mac and outlasted one 5 s wait on a loaded Linux runner while still streaming (debrief.yml run 8377 job 42784,
 * docs/lessons.md). A stream that stays busy with nothing arriving for one wait fails, naming what it shows; the test's own timeout bounds the
 * rest. Each wait past the first is recorded as a `stream-wait` annotation, so a report shows how far past one wait a stream ran.
 */
export async function expectStreamDone(stream: Locator, options: { timeout?: number } = {}): Promise<void> {
  const started = Date.now();
  let seen = await readStreamProgress(stream);
  for (let waits = 1; ; waits++) {
    try {
      await expect(stream, 'the stream has finished (aria-busy is "false")').toHaveAttribute('aria-busy', 'false', options);
      if (waits > 1) test.info().annotations.push({ type: 'stream-wait', description: `done after ${waits} waits, ${Date.now() - started} ms` });
      return;
    } catch (error) {
      const now = await readStreamProgress(stream).catch(() => seen);
      if (!(error instanceof Error) || !streamGrew(seen, now)) {
        if (error instanceof Error) {
          error.message += `\n\nThe stream is stuck: nothing new arrived during wait ${waits} (${Date.now() - started} ms in all; it shows ${now.chars} characters in ${now.elements} elements).`;
        }
        throw error;
      }
      test.info().annotations.push({ type: 'stream-wait', description: `wait ${waits} ended with the stream still arriving: ${seen.chars} to ${now.chars} characters` });
      seen = now;
    }
  }
}

/**
 * Waits until the dock has planned its arrangement for the window's current size (`data-dock-planned`, Debrief's DockService.planned).
 * `timeout` (default: the expect timeout) is for a caller whose wait is part of a longer phase with its own budget, such as an app's first load.
 */
export async function expectDockPlanned(page: Page, options: { timeout?: number } = {}): Promise<void> {
  await expect(page.locator('[data-dock-planned]'), 'the dock has planned for the current window size').toBeAttached(options);
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
