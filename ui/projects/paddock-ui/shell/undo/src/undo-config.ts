import { InjectionToken } from '@angular/core';

/** Entries the undo stack keeps; the oldest are dropped beyond this. */
export const UNDO_LIMIT = 100;
/** Consecutive actions with the same merge key within this many milliseconds become one entry (typing in a search box). */
export const MERGE_WINDOW_MS = 1500;
/** How long the "Done, Undo" toast stays. */
export const UNDO_TOAST_MS = 8000;

/**
 * The undo stack's clock: the time (for merging) and a one-shot timer (for the toast). A port, so tests drive both by hand
 * and no test sleeps or reads the wall clock.
 */
export interface UndoClock {
  now(): number;
  /** Calls `fn` once after `ms`; the returned function cancels it. */
  after(ms: number, fn: () => void): () => void;
}

export const UNDO_CLOCK = new InjectionToken<UndoClock>('PADDOCK_UNDO_CLOCK', {
  providedIn: 'root',
  factory: () => ({
    now: () => Date.now(),
    after: (ms, fn) => {
      const handle = setTimeout(fn, ms);
      return () => clearTimeout(handle);
    },
  }),
});
