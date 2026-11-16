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
}

/**
 * Who moved the scroll position: `own` (the grid's write read back, within 1 px of rounding, or that write clamped because the
 * spacer had not grown yet: the element reports its maximum while the write was beyond it) or `user`. Pure: the same rules for every
 * engine. A browser clamps a write made before the spacer grew and reports the clamped position, so comparing with the written
 * value alone takes the grid's own scroll for the user's.
 */
export function classifyScroll({ read, pendingWrite, lastWrite, maxScroll }: ScrollRead): 'own' | 'user' {
  if (!Number.isFinite(read)) return 'user';
  if (Number.isFinite(lastWrite) && Math.abs(read - lastWrite) <= 1) return 'own';
  if (Number.isFinite(maxScroll) && Math.abs(read - maxScroll) <= 1) {
    for (const written of [pendingWrite, lastWrite]) if (written !== null && Number.isFinite(written) && written > maxScroll + 1) return 'own';
  }
  return 'user';
}
