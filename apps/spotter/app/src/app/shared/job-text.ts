import type { JobProgress } from '../core/api.types';
import { formatBytes } from './format';

/** The numbers of a phase in words: "5.0 MiB of 20.0 MiB", "3 of 12", "12,345 found"; empty when there is nothing to count. */
export function jobNumbers(p: JobProgress): string {
  if (p.unit === 'bytes') return p.total === null ? formatBytes(p.done) : `${formatBytes(p.done)} of ${formatBytes(p.total)}`;
  if (p.total !== null) return `${p.done.toLocaleString()} of ${p.total.toLocaleString()}${p.phase === 'settling' ? ' s' : ''}`;
  return p.phase === 'finding' ? `${p.done.toLocaleString()} found` : '';
}

/** How far a phase is, 0 to 1; `null` when its total is not known (the bar is indeterminate and the count grows). */
export function jobFraction(p: JobProgress | null): number | null {
  if (!p || p.total === null || p.total <= 0) return null;
  return Math.min(1, p.done / p.total);
}
