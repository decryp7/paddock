import type { Confidence, LifeOutcome, LifeSubject } from '../../core/api.types';
import type { IconName } from '../../shared/icons';

export type OutcomeKind = 'remaining' | 'beyondHorizon' | 'noWearDetected' | 'alreadyPastLimit' | 'tooLittleHistory' | 'unavailable';

/** Which outcome the engine reported. */
export function outcomeKind(o: LifeOutcome): OutcomeKind {
  return typeof o === 'string' ? o : (Object.keys(o)[0] as OutcomeKind);
}

export const SUBJECT_ICON: Record<LifeSubject, IconName> = { ssd: 'disk', battery: 'battery' };
export const SUBJECT_WORD: Record<LifeSubject, string> = { ssd: 'disk', battery: 'battery' };

/** Only a limit already passed is a warning; everything else is calm. */
export const outcomeTone = (o: LifeOutcome): 'warn' | 'ok' | 'muted' => {
  const k = outcomeKind(o);
  return k === 'alreadyPastLimit' ? 'warn' : k === 'noWearDetected' || k === 'beyondHorizon' ? 'ok' : 'muted';
};

/** `medium` → `Confidence: medium`. Always words. */
export const confidenceText = (c: Confidence): string => `Confidence: ${c}`;

export interface RangeBar {
  /** Where the soonest estimate sits on the track, 0-100. */
  lowPct: number;
  /** Where the expected value sits, 0-100. */
  midPct: number;
}

/** Maps the three day counts onto a track that runs from now (0) to the latest estimate (100). Layout only. */
export function rangeBar(r: { lowDays: number; midDays: number; highDays: number }): RangeBar {
  const top = Math.max(r.highDays, 1);
  const pct = (d: number) => Math.min(100, Math.max(0, (d / top) * 100));
  return { lowPct: pct(r.lowDays), midPct: pct(r.midDays) };
}

/** How far the history has come toward what the forecast needs, 0-100. */
export function historyProgress(p: { haveDays: number; needDays: number }): number {
  return p.needDays <= 0 ? 100 : Math.min(100, Math.max(0, (p.haveDays / p.needDays) * 100));
}
