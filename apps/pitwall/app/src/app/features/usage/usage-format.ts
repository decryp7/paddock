import type { LimitKind, UsageCounters, UsageRange } from '../../core/api.types';
import { bytes, compact, int } from '../../shared/format';

/** A limit's used or limit value in its own unit: bytes for the memory share, otherwise a count. */
export function limitValue(kind: LimitKind, n: number): string {
  if (kind === 'memoryShare') return bytes(n);
  return kind === 'tokensPerDay' || kind === 'tokensPerMonth' ? compact(n) : int(n);
}

/** The width of a limit's bar: its percentage, capped at the full bar. */
export function barWidth(percent: number): number {
  return Math.max(0, Math.min(100, percent));
}

export const USAGE_RANGES: UsageRange[] = ['24h', '7d', '30d'];

/** The span a range covers, in words, for headings and screen readers. */
export const USAGE_RANGE_TEXT: Record<UsageRange, string> = { '24h': 'the last 24 hours', '7d': 'the last 7 days', '30d': 'the last 30 days' };

/** Prompt plus completion tokens. */
export function tokensOf(c: UsageCounters): number {
  return c.promptTokens + c.completionTokens;
}
