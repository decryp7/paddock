import { formatClock } from './format';

/** What the agent says about a volume's figures: when they were read and how old that is against the refresh interval. */
export interface VolumeAge {
  reachable: boolean;
  measuredAt?: number | null;
  freshness?: 'fresh' | 'late' | 'stale';
}

/** The words for a volume whose figures are not current; `null` when they are (the agent decides, this only words it). */
export function volumeAgeNote(v: VolumeAge): string | null {
  const at = v.measuredAt === null || v.measuredAt === undefined ? null : formatClock(v.measuredAt);
  if (!v.reachable) return at ? `Not answering since ${at}` : 'Not answering';
  if (v.freshness === 'stale') return at ? `Not updating, as of ${at}` : 'Not updating';
  if (v.freshness === 'late') return at ? `As of ${at}` : null;
  return null;
}

/** Whether the row is greyed: figures that are past trust (a stale reading or a volume that does not answer). */
export function volumeIsGreyed(v: VolumeAge): boolean {
  return !v.reachable || v.freshness === 'stale';
}
