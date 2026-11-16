import type { ProcessRef, ProcessSample } from './api.types';
import { readValue } from './measured';

/** Whether two process references name the same process: a pid reused by a new process is `different`; no start time is `unknown`. */
export type Identity = 'same' | 'different' | 'unknown';

/**
 * Compares two references (rule 53, the one place that does it; mirrors `ProcessRef::identity` in dyno-core). The same pid with
 * different start times is another process; the same pid with a start time missing on either side says nothing, so it is `unknown`,
 * never `same`.
 */
export function identify(a: ProcessRef, b: ProcessRef): Identity {
  if (a.pid !== b.pid) return 'different';
  if (a.started === null || b.started === null) return 'unknown';
  return a.started === b.started ? 'same' : 'different';
}

/**
 * Whether a screen may treat `b` as the thing it holds for `a` (a selection, a reply for a request): it may unless the pids or the
 * start times show another process. An unknown start time keeps the earlier behaviour (the pid alone), so a process the OS never
 * gives a start time for can still be selected.
 */
export function notDifferent(a: ProcessRef, b: ProcessRef): boolean {
  return identify(a, b) !== 'different';
}

/**
 * Whether `b` is the process `a` names, for a screen that must not act on another one (a selection, the reply for a request): the
 * same pid and the same start time, or the same pid with no start time on either side (the OS never gave one, so the pid is all
 * there is). A start time on one side only is not enough: that may be a process that reused the pid.
 */
export function isSame(a: ProcessRef, b: ProcessRef): boolean {
  return identify(a, b) === 'same' || (a.pid === b.pid && a.started === null && b.started === null);
}

/** The reference of a process sample (pid and the start time when it was read). */
export function refOf(p: Pick<ProcessSample, 'pid' | 'started'>): ProcessRef {
  return { pid: p.pid, started: readValue(p.started) };
}

/** A stable key for a process in a list or a map: pid plus start time (the pid alone when the OS gave none). */
export function refKey(ref: ProcessRef): string {
  return ref.started === null ? `p${ref.pid}` : `p${ref.pid}@${ref.started}`;
}
