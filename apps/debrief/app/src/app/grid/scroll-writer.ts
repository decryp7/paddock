/**
 * The one place the grid assigns `scrollTop` (ADR 0072, decision 2); `EventsGrid.setTop` is its only caller. Returns the position the
 * element reports after the write (a browser clamps a write that is beyond the maximum). The ESLint rule that keeps other writers out
 * of `src/app/grid` exempts this file.
 */
export function writeScrollTop(el: HTMLElement, physical: number): number {
  // conformance: owned-scroll — the grid's single writer; its scroller is events-grid.ts's .viewport (pdOwnedScroll)
  el.scrollTop = physical;
  return el.scrollTop;
}
