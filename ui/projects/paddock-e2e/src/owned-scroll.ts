import { expect, type Locator } from '@playwright/test';

/** What `scrollAnchoringShift` measured: how far the browser moved `scrollTop` by itself, with anchoring forced on and as styled. */
export interface AnchoringShift {
  /** The shift with `overflow-anchor: auto` forced on the scroller (the positive control: an anchoring browser moves it by `grow`). */
  readonly control: number;
  /** The shift with the scroller's own style: 0 when it makes no scroll moves of its own (family ADR 0105). */
  readonly shift: number;
  /** The `overflow-anchor` the scroller computes with its own style. */
  readonly computed: string;
  /** Where each measurement scrolled to (0 means the element did not scroll: not a scroller, or too short). */
  readonly scrolledTo: readonly number[];
  readonly client: number;
  readonly scrollHeight: number;
}

/**
 * Measures whether the browser moves a scroller's position by itself when content above what is in view grows (CSS scroll anchoring;
 * family ADR 0105). In the page it takes the scroller's own children out of the anchor choice, puts an empty probe and a tall filler at the start of the scroller, scrolls into the filler, grows the
 * probe by `grow` px and lays out: an anchoring browser moves `scrollTop` by `grow` to keep the filler still. It does this twice, first
 * with `overflow-anchor: auto` forced (the positive control, ER-33), then with the scroller's own style, and restores the scroller (its
 * children, inline style and position) afterwards. Pure measurement: it moves nothing the app keeps.
 */
export async function scrollAnchoringShift(scroller: Locator, grow = 100): Promise<AnchoringShift> {
  return scroller.evaluate((el, grow) => {
    const style = el.style.getPropertyValue('overflow-anchor');
    const priority = el.style.getPropertyPriority('overflow-anchor');
    const top = el.scrollTop;
    const probe = document.createElement('div');
    const filler = document.createElement('div');
    for (const d of [probe, filler]) {
      d.setAttribute('aria-hidden', 'true');
      d.style.display = 'block';
      d.style.position = 'static';
      d.style.flex = 'none'; // a flex column (a chat log) would shrink the filler to fit
    }
    filler.style.height = `${el.clientHeight * 4 + 2000}px`;
    // The app's own children leave the anchor choice to the filler (`overflow-anchor: none` on an element takes it and its subtree out of
    // the choice): an engine may otherwise pick an absolutely placed child that the probe does not move, and anchor without moving.
    const others = [...el.children].filter((c): c is HTMLElement => c instanceof HTMLElement).map((c) => [c, c.style.getPropertyValue('overflow-anchor'), c.style.getPropertyPriority('overflow-anchor')] as const);
    for (const [c] of others) c.style.setProperty('overflow-anchor', 'none', 'important');
    el.prepend(probe, filler);
    const scrolled: number[] = [];
    const measure = (): number => {
      probe.style.height = '0px';
      // Into the filler, below its top by one view height: the filler is then the only thing in view, so it is the anchor.
      el.scrollTop = filler.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop + el.clientHeight;
      el.getBoundingClientRect();
      const before = el.scrollTop;
      scrolled.push(before);
      probe.style.height = `${grow}px`;
      el.getBoundingClientRect(); // lay out: an anchoring browser adjusts scrollTop here
      return el.scrollTop - before;
    };
    try {
      el.style.setProperty('overflow-anchor', 'auto', 'important');
      const control = measure();
      if (style) el.style.setProperty('overflow-anchor', style, priority);
      else el.style.removeProperty('overflow-anchor');
      const shift = measure();
      return { control, shift, computed: getComputedStyle(el).overflowAnchor, scrolledTo: scrolled, client: el.clientHeight, scrollHeight: el.scrollHeight };
    } finally {
      if (style) el.style.setProperty('overflow-anchor', style, priority);
      else el.style.removeProperty('overflow-anchor');
      probe.remove();
      for (const [c, v, p] of others) {
        if (v) c.style.setProperty('overflow-anchor', v, p);
        else c.style.removeProperty('overflow-anchor');
      }
      filler.remove();
      el.scrollTop = top;
    }
  }, grow);
}

/**
 * Asserts that a scroller our code positions makes no scroll moves of its own (family ADR 0105): when content above the view grows by
 * 100 px, `scrollTop` stays. Fails as "not measured" when the positive control did not move (the browser does not anchor here, so the
 * check could not see the thing, ER-33).
 */
export async function expectNoScrollMovesOfItsOwn(scroller: Locator, name = 'the scroller'): Promise<void> {
  const r = await scrollAnchoringShift(scroller);
  expect(r.control, `${name}: not measured, the browser did not anchor even with overflow-anchor forced on (scrolled to ${r.scrolledTo.join(', ')}, client ${r.client}, scrollHeight ${r.scrollHeight})`).toBe(100);
  expect(r.shift, `${name}: the browser moved scrollTop by itself when content above the view grew 100 px (overflow-anchor ${r.computed}); put pdOwnedScroll on it`).toBe(0);
}
