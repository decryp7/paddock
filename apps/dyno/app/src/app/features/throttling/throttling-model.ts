import type { Confidence, ThrottleCause, ThrottleEpisode, ThrottleVerdict } from '../../core/api.types';

export type DotTone = 'ok' | 'warn' | 'muted';

/** The word for a cause, as the table shows it. */
export const CAUSE_LABEL: Record<ThrottleCause, string> = {
  thermal: 'Heat',
  power: 'Power limit',
  // The OS's own name for the mode (Low Power Mode, Battery saver) is in the engine's verdict; the table says what kind of cause it is.
  batterySaver: 'Power saving mode',
  unexplained: 'Unexplained',
};

/** Heat and unexplained slowdowns deserve a look; a power limit or the battery saver is the system doing what it was asked. */
export const CAUSE_TONE: Record<ThrottleCause, DotTone> = { thermal: 'warn', power: 'muted', batterySaver: 'muted', unexplained: 'warn' };

/** `high` → `Confidence: high`. Always words. */
export const confidenceText = (c: Confidence): string => `Confidence: ${c}`;

/** The dot beside the engine's headline: a warning only when the CPU is slowed down, a quiet one otherwise. */
export function verdictTone(v: ThrottleVerdict): DotTone {
  if (typeof v === 'object') return 'warn';
  return v === 'fine' ? 'ok' : 'muted';
}

/** The cause when the verdict says the CPU is throttled. */
export function verdictCause(v: ThrottleVerdict): ThrottleCause | null {
  return typeof v === 'object' ? v.throttled.cause : null;
}

export type SortDirection = 'desc' | 'asc';

/** Episodes ordered by start (newest first by default); never mutates the input. */
export function sortEpisodes(episodes: readonly ThrottleEpisode[], direction: SortDirection): ThrottleEpisode[] {
  const sign = direction === 'desc' ? -1 : 1;
  return [...episodes].sort((a, b) => sign * (a.start - b.start));
}

/** How long an episode lasted in seconds; a running one lasts until `now`. */
export const episodeSeconds = (e: ThrottleEpisode, now: number): number => Math.max(0, ((e.end ?? now) - e.start) / 1000);
