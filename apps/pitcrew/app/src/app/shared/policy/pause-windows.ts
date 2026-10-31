import type { PauseWindows } from '@decryp7/paddock-ui/schedule';
import type { UpdateWindow } from '../../core/api.types';

/**
 * Pitcrew's pause windows (config `pause.windows`, always in the computer's local time) as the shared editor's model.
 * The zone is `null` (local) because Pitcrew's policy has none. Pure.
 */
export function toPauseWindows(windows: readonly UpdateWindow[]): PauseWindows {
  return { windows: windows.map((w) => ({ days: [...w.days], start: w.start, end: w.end })), zone: null };
}

/** The shared editor's windows back as Pitcrew's policy windows; the zone is not part of the policy and is dropped. Pure. */
export function fromPauseWindows(value: PauseWindows): UpdateWindow[] {
  return value.windows.map((w) => ({ days: [...w.days], start: w.start, end: w.end }));
}

const WORKDAYS: UpdateWindow['days'] = ['mon', 'tue', 'wed', 'thu', 'fri'];

/**
 * Pitcrew's new pause windows start on Monday to Friday, but the shared editor adds a window with no days. When `next` has
 * exactly one more window than `prev` and it has no days, it is a window the user just added: it gets Monday to Friday, once.
 * Any other change (an edit, a removal, a user who unticks every day later) is returned as it is. Pure.
 */
export function fillAddedWindow(prev: readonly UpdateWindow[], next: UpdateWindow[]): UpdateWindow[] {
  const last = next[next.length - 1];
  if (next.length !== prev.length + 1 || !last || last.days.length > 0) return next;
  return [...next.slice(0, -1), { ...last, days: [...WORKDAYS] }];
}
