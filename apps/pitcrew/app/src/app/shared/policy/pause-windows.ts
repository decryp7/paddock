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
