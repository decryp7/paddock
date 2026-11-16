/** What the grid knows when a `scroll` event arrives (ADR 0072, decision 2). */
export interface ScrollRead {
  /** The scroll position the element reports now. */
  read: number;
  /** A write that waits for the render, or null. */
  pendingWrite: number | null;
  /** The last position the grid wrote (NaN before the first write). */
  lastWrite: number;
  /** The furthest the element can scroll now (`scrollHeight - clientHeight`). */
  maxScroll: number;
  /** An input event (wheel, pointer, touch, key) reached the grid since its last write. */
  recentInput: boolean;
  /** The layout changed since the grid's last write (the total or the viewport height). */
  layoutChanged: boolean;
}

/**
 * Who moved the scroll position: `own` (the grid's write read back, within 1 px of rounding, or that write clamped because the
 * spacer had not grown yet: the element reports its maximum while the write was beyond it), `spurious` (the browser reset it to the top
 * with no input) or `user`. Pure: the same rules for every
 * engine. A browser clamps a write made before the spacer grew and reports the clamped position, so comparing with the written
 * value alone takes the grid's own scroll for the user's.
 */
export function classifyScroll({ read, pendingWrite, lastWrite, maxScroll, recentInput, layoutChanged }: ScrollRead): 'own' | 'user' | 'spurious' {
  if (!Number.isFinite(read)) return 'user';
  if (Number.isFinite(lastWrite) && Math.abs(read - lastWrite) <= 1) return 'own';
  if (Number.isFinite(maxScroll) && Math.abs(read - maxScroll) <= 1) {
    for (const written of [pendingWrite, lastWrite]) if (written !== null && Number.isFinite(written) && written > maxScroll + 1) return 'own';
  }
  // The layout changed since our last write and no input moved the view: the browser adjusted the position itself (WebKit resets or shifts
  // a far scroll position while the spacer or the viewport changes size; found by the e2e). It is not the user; the grid writes its own
  // position back. A scroll with an input before it, or with no layout change, stays the user's (find in page, assistive technology).
  if (!recentInput && layoutChanged) return 'spurious';
  return 'user';
}
