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
  /** The user is in a gesture: an input event (wheel, pointer, touch, key) reached the grid since its last write, or the pointer is held. */
  recentInput: boolean;
  /**
   * The layout is changing right now: the total or the viewport height changed and the frame that renders it has not passed yet, or the
   * element's own height differs from the last write's. It is not "changed at some time since the last write": the total of a view that
   * keeps its position grows all the time without a write.
   */
  layoutChanged: boolean;
  /** How far the furthest scroll position moved since the grid's last write (`|maxScroll - maxScroll at the write|`): the size of the layout change. */
  layoutDelta: number;
}

/**
 * Who moved the scroll position: `own` (the grid's write read back, within 1 px of rounding, or that write clamped because the
 * spacer had not grown yet: the element reports its maximum while the write was beyond it), `spurious` (the browser reset it to the top or shifted it by the
 * layout change, with no input) or `user`. Pure: the same rules for every
 * engine. A browser clamps a write made before the spacer grew and reports the clamped position, so comparing with the written
 * value alone takes the grid's own scroll for the user's.
 */
export function classifyScroll({ read, pendingWrite, lastWrite, maxScroll, recentInput, layoutChanged, layoutDelta }: ScrollRead): 'own' | 'user' | 'spurious' {
  if (!Number.isFinite(read)) return 'user';
  if (Number.isFinite(lastWrite) && Math.abs(read - lastWrite) <= 1) return 'own';
  if (Number.isFinite(maxScroll) && Math.abs(read - maxScroll) <= 1) {
    for (const written of [pendingWrite, lastWrite]) if (written !== null && Number.isFinite(written) && written > maxScroll + 1) return 'own';
  }
  // No input moved the view and the layout changed since our last write: the browser adjusted the position itself. Two shapes, each seen
  // in the e2e: a far position reset to 0, and a shift by about the size of the layout change (the spacer grew by `layoutDelta`). Any other
  // position is not the browser's (a scrollbar drag has pointer input; find in page and assistive technology have none and may go anywhere),
  // so it stays the user's. The grid writes its own position back for the two shapes.
  if (!recentInput && layoutChanged && read <= 1 && Number.isFinite(lastWrite) && lastWrite > 1) return 'spurious';
  if (!recentInput && layoutChanged && layoutDelta > 0 && Number.isFinite(lastWrite) && Math.abs(read - lastWrite) <= layoutDelta + 1) return 'spurious';
  return 'user';
}
