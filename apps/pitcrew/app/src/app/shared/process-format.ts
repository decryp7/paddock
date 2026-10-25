import type { Fate, KeepFor, Leftover, ProcessRule } from '../core/api.types';
import { formatSpan } from './format';

/** Pure wording for the Processes tab and the process policy. The engine decides; this only names it. */

/** `10 min`, `3 h`, `2 d` (the engine's `KeepFor::label`). */
export function keepLabel(k: KeepFor): string {
  return `${k.amount} ${k.unit === 'minutes' ? 'min' : k.unit === 'hours' ? 'h' : 'd'}`;
}

/** The "Keep longer" choices. */
export const KEEP_EXTRAS: { label: string; extra: KeepFor }[] = [
  { label: '30 minutes', extra: { amount: 30, unit: 'minutes' } },
  { label: '1 hour', extra: { amount: 1, unit: 'hours' } },
  { label: '3 hours', extra: { amount: 3, unit: 'hours' } },
  { label: '1 day', extra: { amount: 1, unit: 'days' } },
];

/** What the row says about a leftover's future. `tone` is a hint for the colour; the words always carry the meaning. */
export interface FateText {
  /** `stops in 2 h 14 min`, `never stopped (rule adb)`, … */
  headline: string;
  /** Why: `kept by rule *GradleDaemon*` or `default 10 min`. */
  because: string;
  tone: 'info' | 'warning' | 'neutral';
}

/** How a leftover's rule reads: the matching rule's pattern, or the default keep time. */
export function becauseText(l: Leftover, defaultKeep: KeepFor): string {
  return l.rule ? `kept by rule ${l.rule}` : `default ${keepLabel(defaultKeep)}`;
}

/**
 * The words for a leftover's fate. A stop time in the future is a countdown; one that has passed while a job still
 * runs waits for the job (leftovers are only stopped while the runner is idle).
 */
export function fateText(l: Leftover, now: number, idle: boolean, defaultKeep: KeepFor): FateText {
  const because = becauseText(l, defaultKeep);
  return { ...headline(l.fate, now, idle, l.rule), because };
}

function headline(fate: Fate, now: number, idle: boolean, rule: string | null): Pick<FateText, 'headline' | 'tone'> {
  switch (fate.kind) {
    case 'stopsAt':
      if (fate.at > now) return { headline: `stops in ${formatSpan(fate.at - now)}`, tone: 'info' };
      return idle ? { headline: 'being stopped now', tone: 'warning' } : { headline: 'waits for the job to finish', tone: 'warning' };
    case 'never':
      return { headline: `never stopped${rule ? ` (rule ${rule})` : ''}`, tone: 'neutral' };
    case 'off':
      return { headline: 'automatic cleanup is off', tone: 'neutral' };
  }
}

/** A rule's keep as the editor shows it: `Never` or `3 h`. */
export function ruleKeepLabel(r: ProcessRule): string {
  return r.keep === 'never' ? 'Never stop' : keepLabel(r.keep);
}

/** The cleanup state chip text of the Processes header. */
export function cleanupSummary(cleanup: boolean, keep: KeepFor): string {
  return cleanup ? `Automatic cleanup: on, keeps leftovers ${keepLabel(keep)} after a job` : 'Automatic cleanup: off';
}
