import type { LimitKind, UsageCounters, UsageRange } from '../../core/api.types';
import { bytes, compact, int } from '../../shared/format';

/** A limit's used or limit value in its own unit: bytes for the memory share, otherwise a count. */
export function limitValue(kind: LimitKind, n: number): string {
  if (kind === 'memoryShare') return bytes(n, 'memory');
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

/** Share of cache-eligible requests answered from a cache (0..1); `null` before any request was eligible. */
export function cacheHitRate(c: Pick<UsageCounters, 'cacheHits' | 'cacheMisses'>): number | null {
  const total = c.cacheHits + c.cacheMisses;
  return total > 0 ? c.cacheHits / total : null;
}

/** The hit rate as `42 %`, or an em dash before any eligible request. */
export function cacheHitRateText(c: Pick<UsageCounters, 'cacheHits' | 'cacheMisses'>): string {
  const r = cacheHitRate(c);
  return r === null ? '—' : `${Math.round(r * 100)} %`;
}

/** Tokens the caches saved: prompt tokens reused from the prompt cache plus tokens cache hits skipped. */
export function tokensSaved(c: Pick<UsageCounters, 'prefixTokens' | 'savedTokens'>): number {
  return c.prefixTokens + c.savedTokens;
}
