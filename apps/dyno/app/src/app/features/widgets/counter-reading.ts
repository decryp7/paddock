import type { CounterReading } from '../../core/api.types';
import type { MetricReading } from '../../core/generated/widgets';
import { formatCounter, readingNumber, readingReason } from '../counters/counters-model';

const none = (reason: string): MetricReading => ({ value: null, text: '—', unit: '', percent: null, tone: 'ok', reason });

/** A performance counter as a reading (the engine reads counters separately from the live update; see `CountersStore`). The percent and the tone are the engine's. */
export function counterReading(r: CounterReading | undefined): MetricReading {
  if (!r) return none('Waiting for the first reading.');
  const n = readingNumber(r);
  if (n === null) return none(readingReason(r) ?? 'The counter is not available.');
  const [text, unit] = formatCounter(n, r.unit);
  return { value: n, text, unit, percent: r.percent, tone: r.tone, reason: null };
}
